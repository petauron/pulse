#!/bin/sh
set -eu

[ "$#" -eq 1 ] || { echo "usage: sudo ./deploy/uninstall-systemd.sh service|agent" >&2; exit 2; }
[ "$(id -u)" -eq 0 ] || { echo "uninstaller must run as root" >&2; exit 1; }

component=$1
case "$component" in service|agent) ;; *) echo "component must be service or agent" >&2; exit 2 ;; esac

systemctl disable --now "senulun-$component.service" 2>/dev/null || true
rm -f "/etc/systemd/system/senulun-$component.service"
rm -f "/usr/local/bin/senulun-$component"
rm -f "/opt/senulun/previous-$component"
rm -rf "/opt/senulun/releases/$component"
systemctl daemon-reload

echo "uninstalled senulun-$component binaries and unit"
echo "configuration, account, and state were preserved; remove them manually only after taking a backup"
