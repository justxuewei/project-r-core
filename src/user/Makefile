TARGET := riscv64gc-unknown-none-elf
MODE := release
APP_DIR := src/bin
TARGET_DIR := target/$(TARGET)/$(MODE)
APPS := $(wildcard $(APP_DIR)/*.rs)
ELFS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%, $(APPS))
BINS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%.bin, $(APPS))

OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64

EASY_FS_FUSE_TARGET := $(shell pwd)/../easy-fs-fuse/target/$(MODE)

elf: $(APPS)
	@cargo build --release

binary: elf
	@$(foreach elf, $(ELFS), $(OBJCOPY) $(elf) --strip-all -O binary $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.bin, $(elf));)

build: binary
ifeq (, $(wildcard $(EASY_FS_FUSE_TARGET)/easy-fs-fuse))
	@echo "easy-fs-fuse not found"
	@cd ../easy-fs-fuse && make
endif
	@$(EASY_FS_FUSE_TARGET)/easy-fs-fuse -s $(APP_DIR) -t $(TARGET_DIR)


clean:
	@cargo clean

.PHONY: elf binary build clean
