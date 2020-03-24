#!/bin/fish
set CARGO_TARGET_DIR (cargo metadata --format-version 1 --manifest-path kernel/Cargo.toml | jq -r .target_directory)
set ESP $CARGO_TARGET_DIR/ESP
set ARCH x86_64
export XBUILD_SYSROOT_PATH=$CARGO_TARGET_DIR/sysroot
cargo xbuild --target=$ARCH-unknown-uefi --manifest-path kernel/efiloader/Cargo.toml
cargo xbuild --target=kernel/$ARCH-kernel.json --manifest-path kernel/Cargo.toml --features arch_$ARCH
cargo xbuild --target=drivers/x86_64-pebble-userspace.json --manifest-path drivers/simple_fb/Cargo.toml
cargo xbuild --target=test_process/x86_64-pebble-userspace.json --manifest-path test_process/Cargo.toml
mkdir -p $ESP
function install
 test -f $CARGO_TARGET_DIR/$ARCH-$argv[1] || ln -s $argv[1] $ESP
end
install unknown-uefi/debug/efiloader.efi
ld --gc-sections -T kernel/src/$ARCH/link.ld -o $ESP/kernel.elf $CARGO_TARGET_DIR/$ARCH-kernel/debug/libkernel.a
install pebble-userspace/debug/simple_fb $ESP
install pebble-userspace/debug/test_process $ESP
qemu-system-x86_64 \
 -nodefaults -serial stdio -device isa-debug-exit,iobase=0xf4,iosize=0x04 -vga std \
 -machine q35 --accel tcg,thread=single -smp 3 -m 512M -cpu qemu64,+xsave \
 -drive if=pflash,format=raw,file=/usr/share/edk2-ovmf/OVMF_CODE.fd,readonly=on \
 -drive if=pflash,format=raw,file=$CARGO_TARGET_DIR/OVMF_VARS.fd,readonly=on \
 -drive format=raw,file=fat:rw:$ESP
