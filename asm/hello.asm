; hello.asm - a message on each row of the 16x2 LCD.
; This is the demo built into the firmware (src/demo_program.rs).

        .cpu "6502"
        .enc "ascii"
        .cdef " ~", 32

        .include "lcd6502.inc"

MSG     = $FB                   ; zero-page pointer used by print

        * = $0200

start   sei
        cld
        ldx #$FF
        txs

        lda #$01
        sta LCD_CONTROL         ; clear display, cursor to row 0 col 0

        lda #<line1
        ldy #>line1
        jsr print

        lda #1
        sta LCD_ROW             ; second row
        lda #0
        sta LCD_COL             ; first column
        lda #<line2
        ldy #>line2
        jsr print

done    jmp done                ; the monitor reports this as "Halted"

; Print the zero-terminated string at address A (low byte), Y (high byte).
print   sta MSG
        sty MSG+1
        ldy #0
_loop   lda (MSG),y
        beq _end
        sta LCD_DATA
        iny
        bne _loop
_end    rts

line1   .null "RASPBERRY PICO 2"
line2   .null "HELLO FROM 6502"
