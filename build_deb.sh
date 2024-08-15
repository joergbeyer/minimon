#!/bin/bash

# set VERSION and RELEASE from the .env file
. .env

export PKG_PATH="dist/`lsb_release -is 2>/dev/null`-`lsb_release -rs 2>/dev/null`/main/binary-amd64"
export PKG_NAME="minimonagent-${VERSION:?}-${RELEASE:?}_amd64.deb"


#mkdir minimonagent
install -d minimonagent/usr/bin minimonagent/lib/systemd/system/ minimonagent/usr/share/doc/minimonagent minimonagent/DEBIAN

#cargo build --release --target=x86_64-unknown-linux-musl
cargo build --release

install -d "${PKG_PATH}"
install DEBIAN/control minimonagent/DEBIAN/
install DEBIAN/postinst minimonagent/DEBIAN/
#install -s target/x86_64-unknown-linux-musl/release/minimonagent minimonagent/usr/bin/minimonagent
install -s target/release/minimonagent minimonagent/usr/bin/minimonagent
install -m 0644 minimonagent.service minimonagent/lib/systemd/system
install -m 0644 changelog.Debian minimonagent/usr/share/doc/minimonagent/changelog.Debian
gzip -n -f -9 minimonagent/usr/share/doc/minimonagent/changelog.Debian
install -m 0644 copyright minimonagent/usr/share/doc/minimonagent/

dpkg-deb --root-owner-group --build minimonagent "${PKG_PATH:?}/${PKG_NAME:?}"
