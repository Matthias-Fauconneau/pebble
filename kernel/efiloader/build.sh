#!/bin/sh
XBUILD_SYSROOT_PATH=$CARGO_TARGET_DIR/sysroot cargo xbuild --target x86_64-unknown-uefi
