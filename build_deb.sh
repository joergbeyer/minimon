#!/bin/bash

export DEBEMAIL="joerg.beyer@gmail.com"
export DEBFULLNAME="Joerg Beyer"
export PKG_NAME="minimonagent-`lsb_release -is 2>/dev/null`-`lsb_release -rs 2>/dev/null`"

#mkdir minimonagent
install -d minimonagent/usr/bin minimonagent/lib/systemd/system/ minimonagent/usr/share/doc/minimonagent minimonagent/DEBIAN

#cargo build --release --target=x86_64-unknown-linux-musl
cargo build --release


install DEBIAN/control minimonagent/DEBIAN/
install DEBIAN/postinst minimonagent/DEBIAN/
#install -s target/x86_64-unknown-linux-musl/release/minimonagent minimonagent/usr/bin/minimonagent
install -s target/release/minimonagent minimonagent/usr/bin/minimonagent
install -m 0644 minimonagent.service minimonagent/lib/systemd/system
install -m 0644 changelog.Debian minimonagent/usr/share/doc/minimonagent/changelog.Debian
gzip -n -f -9 minimonagent/usr/share/doc/minimonagent/changelog.Debian
install -m 0644 copyright minimonagent/usr/share/doc/minimonagent/

dpkg-deb --root-owner-group --build "${PKG_NAME:?}""
