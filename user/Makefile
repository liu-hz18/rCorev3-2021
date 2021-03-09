TARGET := riscv64gc-unknown-none-elf
MODE := release
APP_DIR := src/bin
TARGET_DIR := target/$(TARGET)/$(MODE)
BUILD_DIR := build
ELF_DIR := build

APPS := $(wildcard $(APP_DIR)/*.rs)
CH3-APPS := $(wildcard $(APP_DIR)/ch3*.rs)
ELFS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%, $(APPS))
CH3-ELFS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%, $(CH3-APPS))
BINS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%.bin, $(APPS))
CH3-BINS := $(patsubst $(APP_DIR)/%.rs, $(TARGET_DIR)/%.bin, $(CH3-APPS))

OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64

CP := cp
PY := python3

# 在 target/riscv64gc-unknown-none-elf/release 目录下生成一个同名的 ELF 可执行文件
elf: $(APPS)
	@cargo build --release
	@echo $(APPS)
	@echo $(ELFS)
	@echo $(BINS)

ch3-elf: $(CH3-APPS)
	@$(PY) build.py ch3
	@echo $(CH3-APPS)
	@echo $(CH3-ELFS)
	@echo $(CH3-BINS)

# 将生成的 ELF 文件删除所有 ELF header 和符号得到 .bin 后缀的纯二进制镜像文件。它们将被链接 进内核并由内核在合适的时机加载到内存。
binary: elf
	@$(foreach elf, $(ELFS), \
		$(OBJCOPY) $(elf) --strip-all -O binary $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.bin, $(elf)); \
		$(CP) $(elf) $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.elf, $(elf));	\
		$(OBJDUMP) $(elf) -d > $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.asm, $(elf));)

ch3-binary: ch3-elf
	@$(foreach elf, $(CH3-ELFS), \
    		$(OBJCOPY) $(elf) --strip-all -O binary $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.bin, $(elf)); \
    		$(CP) $(elf) $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.elf, $(elf));	\
    		$(OBJDUMP) $(elf) -d > $(patsubst $(TARGET_DIR)/%, $(TARGET_DIR)/%.asm, $(elf));)

pre:
	@rm -rf $(BUILD_DIR)
	@mkdir -p $(BUILD_DIR)/bin/
	@mkdir -p $(BUILD_DIR)/elf/


CH2_TESTS := ch2 ch2t
CH3_COOP_TESTS := ch3
CH3_TESTS_BASE := $(CH2_TESTS) ch3_0 ch3t
CH4_TESTS := ch2 ch3_0 ch4
CH5_TESTS := $(CH4_TESTS) ch5
CH6_TESTS := $(CH5_TESTS) ch6
CH7_TESTS := $(CH6_TESTS) ch7

ch2: binary pre
	@$(foreach t, $(CH2_TESTS), cp $(TARGET_DIR)/$(t)_*.bin $(BUILD_DIR)/bin/;)
	@$(foreach t, $(CH2_TESTS), cp $(TARGET_DIR)/$(t)_*.elf $(BUILD_DIR)/elf/;)

ch3: ch3-binary pre
	@$(foreach t, $(CH3_COOP_TESTS), cp $(TARGET_DIR)/$(t)_*.bin $(BUILD_DIR)/bin/;)
	@$(foreach t, $(CH3_COOP_TESTS), cp $(TARGET_DIR)/$(t)_*.elf $(BUILD_DIR)/elf/;)

build: ch3

clean:
	@cargo clean
	@rm -rf $(BUILD_DIR)

.PHONY: elf binary build clean ch2 ch3
