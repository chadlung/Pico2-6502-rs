; echo.asm - keys typed in the serial terminal appear on the LCD and are
; echoed back.  Enter moves to the start of the other row.

        .cpu "6502"

        .include "lcd6502.inc"

        * = $0200

start   ldx #$FF
        txs
        lda #$01
        sta LCD_CONTROL         ; clear display

wait    lda SERIAL_STATUS
        bpl wait                ; bit 7 clear: nothing received yet
        lda SERIAL_DATA
        cmp #$0D                ; Enter sends CR; turn it into LF
        bne show
        lda #$0A
show    sta LCD_DATA
        sta SERIAL_DATA
        jmp wait
