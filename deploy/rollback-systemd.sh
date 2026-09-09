#!/bin/sh
set -eu

[ "$#" -eq 1 ] || { echo "usage: sudo ./deploy/rollback-systemd.sh service|agent" >&2; exit 2; }
[ "$(id -u)" -eq 0 ] || { echo "rollback must run as root" >&2; exit 1; }

component=$1
case "$component" in service|agent) ;; *) echo "component must be service or agent" >&2; exit 2 ;; esac

current_link=/usr/local/bin/pulse-$component
previous_link=/opt/pulse/previous-$component
[ -L "$current_link" ] && [ -L "$previous_link" ] || { echo "no previous release is recorded" >&2; exit 1; }

current_target=$(readlink "$current_link")
previous_target=$(readlink "$previous_link")
case "$current_target" in
    "/opt/pulse/releases/$component/"*/pulse-"$component") ;;
    *) echo "current release link points outside the managed release directory" >&2; exit 1 ;;
esac
case "$previous_target" in
    "/opt/pulse/releases/$component/"*/pulse-"$component") ;;
    *) echo "previous release link points outside the managed release directory" >&2; exit 1 ;;
esac
[ -x "$previous_target" ] || { echo "previous release binary is missing" >&2; exit 1; }

temporary_link=/usr/local/bin/.pulse-$component.rollback
rm -f "$temporary_link"
ln -s "$previous_target" "$temporary_link"
mv -Tf "$temporary_link" "$current_link"
ln -sfn "$current_target" "$previous_link"
systemctl restart "pulse-$component.service"
echo "rolled pulse-$component back to $previous_target"
