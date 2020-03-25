#!/bin/fish
cd (dirname (status --current-filename))
XBUILD_SYSROOT_PATH=$CARGO_TARGET_DIR/sysroot cargo xbuild --target=x86_64-kernel.json --features arch_x86_64
ld --gc-sections -T src/x86_64/link.ld -o $CARGO_TARGET_DIR/x86_64-kernel/debug/kernel.elf $CARGO_TARGET_DIR/x86_64-kernel/debug/libkernel.a
