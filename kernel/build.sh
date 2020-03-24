#!/bin/fish
XBUILD_SYSROOT_PATH=$CARGO_TARGET_DIR/sysroot cargo xbuild --target=kernel/x86_64-kernel.json --manifest-path kernel/Cargo.toml --features arch_x86_64
ld --gc-sections -T kernel/src/x86_64/link.ld -o $CARGO_TARGET_DIR/x86_64-kernel/debug/kernel.elf $CARGO_TARGET_DIR/x86_64-kernel/debug/libkernel.a
