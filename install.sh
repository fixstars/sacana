#!/bin/sh -eux

install_dir="${INSTALL_DIR:-/opt/sacana}"

basedir=$(dirname "$0")
binary="$basedir/target/release/sacana"
settings="$basedir/settings.json"

if [ ! -e "$settings" ]; then
  echo "put settings.json"
  exit 1
fi

if [ ! -e "$binary" ]; then
  echo "build sacana for release"
  exit 1
fi

systemd_dir=/etc/systemd/system
service_file=sacana.service

if [ -e $systemd_dir/$service_file ]; then
  systemctl stop $service_file
fi

install -m 0700 -d "$install_dir"

install -s -m 0700 "$binary" "$install_dir"
install -m 0600 "$settings" "$install_dir"

if [ ! -e "$systemd_dir/$service_file" ]; then
  install "$basedir/$service_file" "$systemd_dir"
  systemctl enable $service_file
fi

systemctl start $service_file
