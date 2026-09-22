; Built-in two-line LCD demonstration.
        .cpu "6502"
        .enc "ascii"
        .cdef " ~", 32
        .include "lcd6502.inc"

MSG = $FB
        * = $0200
start   sei
        cld
        ldx #$FF
        txs
        lda #$01
        sta LCD_CONTROL
        lda #<line1
        ldy #>line1
        jsr print
        lda #1
        sta LCD_ROW
        lda #0
        sta LCD_COL
        lda #<line2
        ldy #>line2
        jsr print
done    jmp done
print   sta MSG
        sty MSG+1
        ldy #0
loop    lda (MSG),y
        beq end
        sta LCD_DATA
        iny
        bne loop
end     rts
line1   .null "RASPBERRY PICO 2"
line2   .null "HELLO FROM 6502"

