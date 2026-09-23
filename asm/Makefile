# Assemble the example programs with 64tass.
#
#   make          build hello.bin, echo.bin and blackjack.bin
#   make demo     regenerate the firmware's built-in demo from hello.asm

AS      = 64tass
ASFLAGS = -a --m6502 --nostart

all: hello.bin echo.bin blackjack.bin

%.bin: %.asm lcd6502.inc
	$(AS) $(ASFLAGS) -L $*.lst -o $@ $<

demo: hello.bin
	python3 ../tools/bin2rs.py hello.bin 0200 > ../src/demo_program.rs

clean:
	rm -f *.bin *.lst

.PHONY: all demo clean
