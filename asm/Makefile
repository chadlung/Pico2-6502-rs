AS = 64tass
ASFLAGS = -a --m6502 --nostart

all: hello.bin echo.bin

%.bin: %.asm lcd6502.inc
	$(AS) $(ASFLAGS) -L $*.lst -o $@ $<

clean:
	rm -f *.bin *.lst

.PHONY: all clean

