; Echo USB serial input to the LCD and terminal.
        .cpu "6502"
        .include "lcd6502.inc"
        * = $0200
start   ldx #$FF
        txs
        lda #$01
        sta LCD_CONTROL
wait    lda SERIAL_STATUS
        bpl wait
        lda SERIAL_DATA
        cmp #$0D
        bne show
        lda #$0A
show    sta LCD_DATA
        sta SERIAL_DATA
        jmp wait

