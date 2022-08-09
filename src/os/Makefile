CURDIR := $(shell pwd)
RUST_TARGET := riscv64gc-unknown-none-elf

BOOTLOADER_DIR := ../../bootloader
TARGET_DIR := target
USER_DIR := ../user/target

QEMU_BOOTLOADER := $(BOOTLOADER_DIR)/rustsbi-qemu.bin

OS_OUTPUT := $(TARGET_DIR)/$(RUST_TARGET)/release/os
OS_BIN_OUTPUT := $(TARGET_DIR)/$(RUST_TARGET)/release/os.bin

FILESYSTEM_IMG := $(USER_DIR)/$(RUST_TARGET)/release/fs.img

build: $(OS_OUTPUT) $(OS_BIN_OUTPUT)
run: qemu
brun: clean run
debug: qemu-gdb
bdebug: clean debug

# If you want to terminate the QEMU, please
# press `ctrl + A, X`.
qemu-gdb: $(OS_OUTPUT) $(OS_BIN_OUTPUT)
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(QEMU_BOOTLOADER) \
		-device loader,file=$(OS_BIN_OUTPUT),addr=0x80200000 \
		-drive file=$(FILESYSTEM_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
		-s -S

qemu: $(OS_OUTPUT) $(OS_BIN_OUTPUT)
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(QEMU_BOOTLOADER) \
		-device loader,file=$(OS_BIN_OUTPUT),addr=0x80200000 \
		-drive file=$(FILESYSTEM_IMG),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0

gdb: $(OS_OUTPUT)
	@riscv64-unknown-elf-gdb \
		-ex "file $(OS_OUTPUT)" \
		-ex "set arch riscv:rv64" \
		-ex "target remote localhost:1234"

.DEFAULT: default

$(OS_OUTPUT):
	@cargo build --release

$(OS_BIN_OUTPUT): $(OS_OUTPUT)
	@rust-objcopy --strip-all $< -O binary $@

.PHONY: \
	clean

clean: 
	@cargo clean
