# Building
TARGET := riscv64gc-unknown-none-elf
MODE := release
KERNEL_ELF := target/$(TARGET)/$(MODE)/os
KERNEL_BIN := $(KERNEL_ELF).bin
DISASM_TMP := target/$(TARGET)/$(MODE)/asm
DEBUG_KERNEL_ELF := target/$(TARGET)/debug/os
DEBUG_KERNEL_BIN := $(DEBUG_KERNEL_ELF).bin

# BOARD
BOARD ?= qemu
SBI ?= rustsbi
BOOTLOADER := ../bootloader/$(SBI)-$(BOARD).bin
K210_BOOTLOADER_SIZE := 131072

# KERNEL ENTRY
ifeq ($(BOARD), qemu)
	KERNEL_ENTRY_PA := 0x80200000
else ifeq ($(BOARD), k210)
	KERNEL_ENTRY_PA := 0x80020000
endif

# Run K210
K210-SERIALPORT	= /dev/ttyUSB0
K210-BURNER = ../tools/kflash.py

# Binutils
OBJDUMP := rust-objdump --arch-name=riscv64
OBJCOPY := rust-objcopy --binary-architecture=riscv64

# Binutils
# 反汇编ELF文件
OBJDUMP := rust-objdump --arch-name=riscv64
# 当前的ELF执行程序有许多与执行无直接关系的信息（如调试信息等），可以通过 rust-objcopy 工具来清除
OBJCOPY := rust-objcopy --binary-architecture=riscv64
# 看看 ELF 文件中究竟包含什么内容
READOBJ := rust-readobj -all
# addr2line 的默认 addr
ADDR := 0x8002000c

# Disassembly
DISASM ?= -x -D

build: env $(KERNEL_BIN)

build-debug: env $(DEBUG_KERNEL_BIN)

env:
	(rustup target list | grep "riscv64gc-unknown-none-elf (installed)") || rustup target add $(TARGET)
	cargo install cargo-binutils
	rustup component add rust-src
	rustup component add llvm-tools-preview

$(KERNEL_BIN): kernel
	@$(OBJCOPY) $(KERNEL_ELF) --strip-all -O binary $@

$(DEBUG_KERNEL_BIN): kernel-debug
	@$(OBJCOPY) $(DEBUG_KERNEL_ELF) --strip-all -O binary $@

kernel:
	@cd ../user && make build
	@echo Platform: $(BOARD)
	@cp src/linker-$(BOARD).ld src/linker.ld
	@cargo build --release
	@rm src/linker.ld

kernel-debug:
	@echo Platform: $(BOARD)
	@cp src/linker-$(BOARD).ld src/linker.ld
	@cargo build
	@rm src/linker.ld

clean:
	@cargo clean

disasm: kernel
	@$(OBJDUMP) $(DISASM) $(KERNEL_ELF) | less

# 显示debug版本的反汇编
disasm-debug:
	@$(OBJDUMP) $(DISASM) $(DEBUG_KERNEL_ELF) | less

disasm-vim: kernel
	@$(OBJDUMP) $(DISASM) $(KERNEL_ELF) > $(DISASM_TMP)
	@vim $(DISASM_TMP)
	@rm $(DISASM_TMP)

# 看看 ELF 文件中究竟包含什么内容
readobj: kernel
	$(READOBJ) $(KERNEL_ELF) | less

readobj-debug: kernel-debug
	$(READOBJ) $(DEBUG_KERNEL_ELF) | less

# 查看文件配置/元信息
file: kernel
	file $(KERNEL_ELF)

file-debug: kernel-debug
	file $(DEBUG_KERNEL_ELF)

# 查看地址所在函数段, e.g. `make addr2line ADDR=0x80020d28`
addr2line:
	addr2line $(ADDR) -f -a -p -C -e $(DEBUG_KERNEL_ELF)

run: tools run-inner

tools:
	(which $(K210-BURNER)) || (cd .. && git clone https://github.com/sipeed/kflash.py.git && mv kflash.py tools)

run-inner: build
ifeq ($(BOARD),qemu)
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA)
else
	@cp $(BOOTLOADER) $(BOOTLOADER).copy
	@dd if=$(KERNEL_BIN) of=$(BOOTLOADER).copy bs=$(K210_BOOTLOADER_SIZE) seek=1
	@mv $(BOOTLOADER).copy $(KERNEL_BIN)
	@sudo chmod 777 $(K210-SERIALPORT)
	python3 $(K210-BURNER) -p $(K210-SERIALPORT) -b 1500000 $(KERNEL_BIN)
	python3 -m serial.tools.miniterm --eol LF --dtr 0 --rts 0 --filter direct $(K210-SERIALPORT) 115200
endif

# 编译运行debug版本
run-debug: tools build-debug
ifeq ($(BOARD),qemu)
	@qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(DEBUG_KERNEL_BIN),addr=$(KERNEL_ENTRY_PA)
else
	@cp $(BOOTLOADER) $(BOOTLOADER).copy
	@dd if=$(DEBUG_KERNEL_BIN) of=$(BOOTLOADER).copy bs=128K seek=1
	@mv $(BOOTLOADER).copy $(DEBUG_KERNEL_BIN)
	@sudo chmod 777 $(K210-SERIALPORT)
	python3 $(K210-BURNER) -p $(K210-SERIALPORT) -b 1500000 $(DEBUG_KERNEL_BIN)
	python3 -m serial.tools.miniterm --eol LF --dtr 0 --rts 0 --filter direct $(K210-SERIALPORT) 115200
endif

debug: build
	@tmux new-session -d \
		"qemu-system-riscv64 -machine virt -nographic -bios $(BOOTLOADER) -device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA) -s -S" && \
		tmux split-window -h "riscv64-unknown-elf-gdb -ex 'file $(KERNEL_ELF)' -ex 'set arch riscv:rv64' -ex 'target remote localhost:1234'" && \
		tmux -2 attach-session -d

.PHONY: build env kernel clean disasm disasm-vim run-inner tools
