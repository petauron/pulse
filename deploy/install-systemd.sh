#!/bin/sh
set -eu

usage() {
    echo "usage: sudo ./deploy/install-systemd.sh service|agent /absolute/path/to/senulun-BINARY" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
[ "$(id -u)" -eq 0 ] || { echo "installer must run as root" >&2; exit 1; }

component=$1
binary=$2
case "$component" in
    service) account=senulun ; state_directory=/var/lib/senulun ; legacy_state_directory=/var/lib/pulse ;;
    agent) account=senulun-agent ; state_directory=/var/lib/senulun-agent ; legacy_state_directory=/var/lib/pulse-agent ;;
    *) usage ;;
esac

[ "${binary#/}" != "$binary" ] || { echo "binary path must be absolute" >&2; exit 1; }
[ -f "$binary" ] && [ -x "$binary" ] || { echo "binary must be a regular executable file" >&2; exit 1; }

version_output=$("$binary" --version)
case "$version_output" in
    "senulun-$component "*) ;;
    *) echo "binary identity does not match senulun-$component" >&2; exit 1 ;;
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
release_directory=/opt/senulun/releases/$component/$version-$binary_sha256
target_binary=$release_directory/senulun-$component
current_link=/usr/local/bin/senulun-$component
previous_link=/opt/senulun/previous-$component
temporary_link=/usr/local/bin/.senulun-$component.new

# This installer is not a migration tool. A fresh Senulun unit beside a legacy
# Pulse unit could create two database writers or enroll the same host twice.
if [ ! -L "$current_link" ] && [ ! -e "$current_link" ]; then
    if [ -e "/etc/systemd/system/pulse-$component.service" ] ||
       [ -e "$legacy_state_directory" ]; then
        echo "legacy Pulse resources exist; use the reviewed stop-and-migrate procedure" >&2
        exit 1
    fi
    if [ "$component" = agent ] && [ -e /etc/systemd/system/vastora-pulse-agent.service ]; then
        echo "managed Pulse Agent exists; migrate it through Vastora" >&2
        exit 1
    fi
fi

old_target=
if [ -L "$current_link" ]; then
    old_target=$(readlink "$current_link")
    case "$old_target" in
        "/opt/senulun/releases/$component/"*/senulun-"$component") ;;
        *) echo "$current_link points outside the managed release directory" >&2; exit 1 ;;
    esac
elif [ -e "$current_link" ]; then
    echo "$current_link exists and is not a symbolic link" >&2
    exit 1
fi

if [ "$component" = agent ]; then
    echo "Privacy notice: without a manual region, Agent country discovery defaults to GeoJS."
    echo "GeoJS sees public egress IP and Agent version, not Senulun credentials or metrics."
    echo "Set SENULUN_GEOIP_PROVIDER=disabled in /etc/senulun/agent.env before upgrading to opt out."
    echo "Existing configuration is preserved; an active Agent is restarted during upgrade."
fi

if ! getent group "$account" >/dev/null 2>&1; then
    groupadd --system "$account"
fi
if ! id "$account" >/dev/null 2>&1; then
    useradd --system --gid "$account" --home-dir "$state_directory" --shell /usr/sbin/nologin "$account"
fi
install --directory --owner "$account" --group "$account" --mode 0700 "$state_directory"
install --directory --owner root --group root --mode 0755 \
    /etc/senulun /opt/senulun /opt/senulun/releases "/opt/senulun/releases/$component" "$release_directory"
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

install --owner root --group root --mode 0644 "$script_directory/senulun-$component.service" "/etc/systemd/system/senulun-$component.service"
if [ ! -e "/etc/senulun/$component.env" ]; then
    install --owner root --group root --mode 0600 "$script_directory/$component.env.example" "/etc/senulun/$component.env"
fi

systemctl daemon-reload
systemctl enable "senulun-$component.service"
if systemctl is-active --quiet "senulun-$component.service"; then
    systemctl restart "senulun-$component.service"
fi

echo "installed senulun-$component $version ($binary_sha256)"
echo "review /etc/senulun/$component.env, then run: systemctl start senulun-$component"
