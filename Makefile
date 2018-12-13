export ARCH ?= x86_64
export BUILD_DIR ?= $(abspath ./build)

.PHONY: prepare bootloader kernel clean qemu gdb update fmt

pebble.img: prepare bootloader kernel
	# Create a temporary image for the FAT partition
	dd if=/dev/zero of=$(BUILD_DIR)/fat.img bs=1M count=64
	mkfs.vfat -F 32 $(BUILD_DIR)/fat.img -n BOOT
	# Copy the stuff into the FAT image
	mcopy -i $(BUILD_DIR)/fat.img -s $(BUILD_DIR)/fat/* ::
	# Create the real image
	dd if=/dev/zero of=$@ bs=512 count=93750
	# Create GPT headers and a single EFI partition
	parted $@ -s -a minimal mklabel gpt
	parted $@ -s -a minimal mkpart EFI FAT32 2048s 93716s
	parted $@ -s -a minimal toggle 1 boot
	# Copy the data from efi.img into the correct place
	dd if=$(BUILD_DIR)/fat.img of=$@ bs=512 count=91669 seek=2048 conv=notrunc
	rm $(BUILD_DIR)/fat.img

prepare:
	@mkdir -p $(BUILD_DIR)/fat/EFI/BOOT

bootloader:
	cargo xbuild --release --target bootloader/uefi_x64.json --manifest-path bootloader/Cargo.toml
	cp bootloader/target/uefi_x64/release/bootloader.efi $(BUILD_DIR)/fat/EFI/BOOT/BOOTX64.efi

kernel:
	cargo xbuild --target=kernel/src/$(ARCH)/$(ARCH)-kernel.json --manifest-path kernel/Cargo.toml --features $(ARCH)
	ld --gc-sections -T kernel/src/$(ARCH)/link.ld -o $(BUILD_DIR)/fat/kernel.elf kernel/target/$(ARCH)-kernel/debug/libkernel.a

clean:
	cd bootloader && cargo clean
	cd kernel && cargo clean
	rm -rf build pebble.iso

update:
	cargo update --manifest-path bootloader/Cargo.toml
	cargo update --manifest-path kernel/Cargo.toml
	cargo update --manifest-path kernel/x86_64/Cargo.toml
	cargo update --manifest-path x86_64/Cargo.toml
	cargo update --manifest-path libmessage/Cargo.toml

fmt:
	@# `cargo fmt` doesn't play nicely with conditional compilation, so we manually `rustfmt` the kernel
	find kernel/src -type f -name "*.rs" -exec rustfmt {} +
	cd bootloader && cargo fmt
	cd x86_64 && cargo fmt
	cd libmessage && cargo fmt
	cd userboot && cargo fmt

doc:
	CARGO_TARGET_DIR=./doc_target cargo doc \
		--all-features \
		--manifest-path kernel/Cargo.toml \
		--document-private-items
	mv doc_target/doc docs
	rm -r doc_target

qemu: pebble.img
	qemu-system-x86_64 \
		-enable-kvm \
		-smp 2 \
		-usb \
		-device usb-ehci,id=ehci \
		--no-reboot \
		--no-shutdown \
		-drive if=pflash,format=raw,file=bootloader/ovmf/OVMF_CODE.fd,readonly \
		-drive if=pflash,format=raw,file=bootloader/ovmf/OVMF_VARS.fd,readonly \
		-drive format=raw,file=$<,if=ide \
		-net none

debug: pebble.img
	qemu-system-x86_64 \
		-d int \
		-smp 2 \
		-usb \
		-device usb-ehci,id=ehci \
		--no-reboot \
		--no-shutdown \
		-drive if=pflash,format=raw,file=bootloader/ovmf/OVMF_CODE.fd,readonly \
		-drive if=pflash,format=raw,file=bootloader/ovmf/OVMF_VARS.fd,readonly \
		-drive format=raw,file=$<,if=ide \
		-net none
