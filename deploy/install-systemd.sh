#!/bin/sh
set -eu

usage() {
    echo "usage: sudo ./deploy/install-systemd.sh service|agent /absolute/path/to/pulse-BINARY" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
[ "$(id -u)" -eq 0 ] || { echo "installer must run as root" >&2; exit 1; }

component=$1
binary=$2
case "$component" in
    service) account=pulse ; state_directory=/var/lib/pulse ;;
    agent) account=pulse-agent ; state_directory=/var/lib/pulse-agent ;;
    *) usage ;;
esac

[ "${binary#/}" != "$binary" ] || { echo "binary path must be absolute" >&2; exit 1; }
[ -f "$binary" ] && [ -x "$binary" ] || { echo "binary must be a regular executable file" >&2; exit 1; }

version_output=$("$binary" --version)
case "$version_output" in
    "pulse-$component "*) ;;
    *) echo "binary identity does not match pulse-$component" >&2; exit 1 ;;
esac
version=${version_output#* }
case "$version" in
    ""|*[!A-Za-z0-9._+-]*) echo "binary returned an unsafe version" >&2; exit 1 ;;
esac
binary_sha256=$(sha256sum -- "$binary" | awk 'NR == 1 { print $1 }')
[ "${#binary_sha256}" -eq 64 ] || { echo "could not determine binary SHA-256" >&2; exit 1; }
case "$binary_sha256" in
    *[!0-9a-f]*) echo "binary SHA-256 has an invalid format" >&2; exit 1 ;;
esac

script_directory=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
release_directory=/opt/pulse/releases/$component/$version-$binary_sha256
target_binary=$release_directory/pulse-$component
current_link=/usr/local/bin/pulse-$component
previous_link=/opt/pulse/previous-$component
temporary_link=/usr/local/bin/.pulse-$component.new

old_target=
if [ -L "$current_link" ]; then
    old_target=$(readlink "$current_link")
    case "$old_target" in
        "/opt/pulse/releases/$component/"*/pulse-"$component") ;;
        *) echo "$current_link points outside the managed release directory" >&2; exit 1 ;;
    esac
elif [ -e "$current_link" ]; then
    echo "$current_link exists and is not a symbolic link" >&2
    exit 1
fi

if ! getent group "$account" >/dev/null 2>&1; then
    groupadd --system "$account"
fi
if ! id "$account" >/dev/null 2>&1; then
    useradd --system --gid "$account" --home-dir "$state_directory" --shell /usr/sbin/nologin "$account"
fi
install --directory --owner "$account" --group "$account" --mode 0700 "$state_directory"
install --directory --owner root --group root --mode 0755 \
    /etc/pulse /opt/pulse /opt/pulse/releases "/opt/pulse/releases/$component" "$release_directory"
if [ -e "$target_binary" ]; then
    cmp -s "$binary" "$target_binary" || {
        echo "$target_binary does not match its content-addressed release path" >&2
        exit 1
    }
else
    install --owner root --group root --mode 0755 "$binary" "$target_binary"
fi

if [ "$old_target" != "$target_binary" ]; then
    if [ -n "$old_target" ]; then
        ln -sfn "$old_target" "$previous_link"
    fi
    rm -f "$temporary_link"
    ln -s "$target_binary" "$temporary_link"
    mv -Tf "$temporary_link" "$current_link"
fi

install --owner root --group root --mode 0644 "$script_directory/pulse-$component.service" "/etc/systemd/system/pulse-$component.service"
if [ ! -e "/etc/pulse/$component.env" ]; then
    install --owner root --group root --mode 0600 "$script_directory/$component.env.example" "/etc/pulse/$component.env"
fi

systemctl daemon-reload
systemctl enable "pulse-$component.service"
if systemctl is-active --quiet "pulse-$component.service"; then
    systemctl restart "pulse-$component.service"
fi

echo "installed pulse-$component $version ($binary_sha256)"
echo "review /etc/pulse/$component.env, then run: systemctl start pulse-$component"
