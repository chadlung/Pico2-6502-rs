; blackjack.asm - blackjack on the 16x2 LCD, played from the serial terminal.
;
; The top row shows the dealer's cards and the bottom row yours, with the
; totals at the right:
;
;     D:K?
;     P:A7          18
;
; Keys: H = hit, S = stand, any key deals the next hand.  Card ranks show as
; A 2-9 T J Q K.  One 52-card deck is used and reshuffled when fewer than 15
; cards remain.  The dealer draws to 17 and stands on all 17s.  The terminal
; also shows each hand and a running score.
;
; The cards depend on when you press keys.  For a repeatable game, set the
; random seed and the fixed-seed flag first:   w 00F0 34 12 01   then   g 200
; (w 00F2 00 switches back to normal play).

        .cpu "6502"
        .enc "ascii"
        .cdef " ~", 32

        .include "lcd6502.inc"

; ---- zero page ---------------------------------------------------------------
COUNT   = $10           ; cards in each hand: COUNT+PLAYER, COUNT+DEALER
DECKPOS = $12           ; index of the next card to deal from DECK
HIDE    = $13           ; nonzero while the dealer's second card is face down
OUT     = $14           ; where putc prints: $00 = LCD, $80 = serial port
PTOT    = $15           ; hand totals
DTOT    = $16
RESULT  = $17           ; outcome of the hand, one of RES_*
WINS    = $18           ; score; indexed from WINS by the score_of table
LOSSES  = $19
PUSHES  = $1A
SUM     = $1B           ; scratch
ACES    = $1C
CNT     = $1D
IDX     = $1E
TMP     = $1F
LIMIT   = $20
MASK    = $21
XSAVE   = $22
STEPS   = $23
PTR     = $E0           ; string pointer used by puts
RNG     = $F0           ; 16-bit random number state, $F0-$F1
FIXED   = $F2           ; nonzero: keypress timing doesn't affect the cards

; ---- data areas --------------------------------------------------------------
DECK    = $1000         ; 52 cards, each a rank 1 (ace) to 13 (king)
HANDS   = $1100         ; the player's cards from $1100, the dealer's from $1110

PLAYER  = 0             ; hand numbers, passed in X
DEALER  = 1

RES_LOSE  = 0           ; hand outcomes
RES_WIN   = 1
RES_PUSH  = 2
RES_BJ    = 3           ; player has blackjack
RES_BUST  = 4           ; player went over 21
RES_DBUST = 5           ; dealer went over 21

say     .macro          ; print a zero-terminated string
        lda #<\1
        ldy #>\1
        jsr puts
        .endm

        * = $0200

start   sei
        cld
        ldx #$FF
        txs
        lda RNG                 ; the generator must not start at zero
        ora RNG+1
        bne +
        lda #$E1
        sta RNG
        lda #$AC
        sta RNG+1
+       lda #0
        sta WINS
        sta LOSSES
        sta PUSHES
        lda #52                 ; deck used up: shuffle before the first hand
        sta DECKPOS

        jsr to_lcd
        lda #$01
        sta LCD_CONTROL
        #say title1
        ldx #1
        ldy #0
        jsr lcd_at
        #say title2
        jsr to_serial
        #say intro
        jsr getkey

; ---- one hand ----------------------------------------------------------------
hand    lda DECKPOS             ; reshuffle when fewer than 15 cards remain
        cmp #52-15+1
        bcc +
        jsr shuffle
+       lda #0
        sta COUNT+PLAYER
        sta COUNT+DEALER
        lda #1
        sta HIDE
        ldx #PLAYER
        jsr add_card
        ldx #DEALER
        jsr add_card
        ldx #PLAYER
        jsr add_card
        ldx #DEALER
        jsr add_card
        jsr totals
        jsr show
        lda PTOT                ; a two-card 21 ends the hand at once
        cmp #21
        beq +
        lda DTOT
        cmp #21
        bne turn
+       jmp natural

turn    lda PTOT                ; 21 stands automatically
        cmp #21
        bcs dealer_turn
        jsr to_serial
        #say prompt
        jsr getkey
        pha
        jsr putc                ; echo the key
        lda #10
        jsr putc
        pla
        cmp #'H'
        beq hit
        cmp #'S'
        beq dealer_turn
        bne turn

hit     ldx #PLAYER
        jsr add_card
        jsr totals
        jsr show
        lda PTOT
        cmp #22
        bcc turn
        lda #0                  ; bust: show the dealer's card, hand over
        sta HIDE
        jsr show
        lda #RES_BUST
        jmp finish

; Turn the dealer's card over, then draw to 17.
dealer_turn
        lda #0
        sta HIDE
        jsr show
_draw   lda DTOT
        cmp #17
        bcs compare
        lda #2
        jsr delay
        ldx #DEALER
        jsr add_card
        jsr totals
        jsr show
        jmp _draw

compare lda DTOT
        cmp #22
        bcc +
        lda #RES_DBUST
        jmp finish
+       lda PTOT
        cmp DTOT
        beq _push
        bcc _lose
        lda #RES_WIN
        jmp finish
_push   lda #RES_PUSH
        jmp finish
_lose   lda #RES_LOSE
        jmp finish

natural lda #0                  ; someone has 21 with two cards
        sta HIDE
        jsr show
        lda PTOT
        cmp #21
        bne _dealer
        lda DTOT
        cmp #21
        beq _push
        lda #RES_BJ
        jmp finish
_push   lda #RES_PUSH
        jmp finish
_dealer lda #RES_LOSE

finish  sta RESULT
        jsr to_lcd
        lda RESULT
        cmp #RES_DBUST
        bne +
        ldx #0                  ; the dealer's row says BUST
        ldy #12
        jsr lcd_at
        #say w_bust
+       ldx #1                  ; the player's row shows the outcome
        ldy #12
        jsr lcd_at
        lda RESULT
        asl a
        tax
        lda words,x
        ldy words+1,x
        jsr puts

        ldx RESULT              ; add it to the score
        lda score_of,x
        tax
        inc WINS,x

        jsr to_serial
        lda RESULT
        asl a
        tax
        lda messages,x
        ldy messages+1,x
        jsr puts
        #say s_wins
        lda WINS
        jsr print_num
        #say s_losses
        lda LOSSES
        jsr print_num
        #say s_pushes
        lda PUSHES
        jsr print_num
        #say s_next
        jsr getkey
        jmp hand

; ---- cards and hands -----------------------------------------------------------

; Fill the deck with four of each rank and shuffle it (Fisher-Yates).
shuffle ldx #0
        lda #1
        sta TMP
_fill   ldy #4
-       lda TMP
        sta DECK,x
        inx
        dey
        bne -
        inc TMP
        lda TMP
        cmp #14
        bne _fill
        ldx #51
_mix    txa                     ; swap card X with a random card 0..X
        jsr random
        tay
        lda DECK,x
        pha
        lda DECK,y
        sta DECK,x
        pla
        sta DECK,y
        dex
        bne _mix
        stx DECKPOS
        jsr to_serial
        #say s_shuffle
        rts

; Deal the next card into hand X.  Keeps X.
add_card
        jsr draw
        pha
        lda BASE,x
        clc
        adc COUNT,x
        tay
        pla
        sta HANDS,y
        inc COUNT,x
        rts

; Take the next card from the deck into A, reshuffling if it has run out.
; Keeps X.
draw    lda DECKPOS
        cmp #52
        bcc +
        txa
        pha
        jsr shuffle
        pla
        tax
+       ldy DECKPOS
        lda DECK,y
        inc DECKPOS
        rts

totals  ldx #PLAYER
        jsr total
        sta PTOT
        ldx #DEALER
        jsr total
        sta DTOT
        rts

; Best total of hand X in A: aces count 1, or 11 if that doesn't go over 21.
total   lda #0
        sta SUM
        sta ACES
        lda COUNT,x
        sta CNT
        beq _soft
        ldy BASE,x
_loop   lda HANDS,y
        cmp #1
        bne +
        inc ACES
+       cmp #10                 ; tens and picture cards count 10
        bcc +
        lda #10
+       clc
        adc SUM
        sta SUM
        iny
        dec CNT
        bne _loop
_soft   lda ACES
        beq _done
        lda SUM
        cmp #12
        bcs _done
        adc #10                 ; carry is clear here
        sta SUM
_done   lda SUM
        rts

; ---- display -----------------------------------------------------------------

; Redraw both hands on the LCD and print them to the terminal.
show    jsr to_lcd
        lda #$01
        sta LCD_CONTROL
        #say s_d
        ldx #DEALER
        jsr cards
        lda HIDE
        bne +
        lda DTOT
        ldx #0
        jsr field
+       ldx #1
        ldy #0
        jsr lcd_at
        #say s_p
        ldx #PLAYER
        jsr cards
        lda PTOT
        ldx #1
        jsr field

        jsr to_serial
        #say s_dealer
        ldx #DEALER
        jsr cards
        lda HIDE
        bne +
        lda DTOT
        jsr equals
+       lda #10
        jsr putc
        #say s_you
        ldx #PLAYER
        jsr cards
        lda PTOT
        jsr equals
        lda #10
        jmp putc

; Print the ranks in hand X, with ? for a face-down dealer card.
cards   lda COUNT,x
        sta CNT
        lda #0
        sta IDX
_loop   lda IDX
        cmp CNT
        bcs _done
        cpx #DEALER
        bne _show
        cmp #1
        bne _show
        lda HIDE
        beq _show
        jsr space
        lda #'?'
        jmp putc
_show   jsr space
        lda BASE,x
        clc
        adc IDX
        tay
        lda HANDS,y
        tay
        lda ranks-1,y
        jsr putc
        inc IDX
        bne _loop
_done   rts

; Between cards, print a space on the terminal (the LCD has no room).
space   lda IDX
        beq +
        bit OUT
        bpl +
        lda #' '
        jmp putc
+       rts

; Print total A right-aligned in columns 13-15 of LCD row X.
field   pha
        ldy #13
        jsr lcd_at
        lda #' '
        jsr putc
        pla
        cmp #10
        bcs +
        pha
        lda #' '
        jsr putc
        pla
+       jmp print_num

; Print " = " and the number in A.
equals  pha
        #say s_eq
        pla
        jmp print_num

; Print A (0-255) in decimal without leading zeros.  Keeps X.
print_num
        stx XSAVE
        ldx #$FF
        sec
-       inx                     ; X = hundreds
        sbc #100
        bcs -
        adc #100
        sta TMP
        ldy #0                  ; Y becomes 1 once a digit is printed
        txa
        beq +
        jsr digit
+       ldx #$FF
        lda TMP
        sec
-       inx                     ; X = tens
        sbc #10
        bcs -
        adc #10
        sta TMP
        txa
        bne +
        cpy #0
        beq _ones
+       jsr digit
_ones   lda TMP
        jsr digit
        ldx XSAVE
        rts

digit   ora #'0'
        jsr putc
        ldy #1
        rts

lcd_at  stx LCD_ROW             ; cursor to row X, column Y
        sty LCD_COL
        rts

to_lcd  lda #$00
        sta OUT
        rts

to_serial
        lda #$80
        sta OUT
        rts

putc    bit OUT                 ; print A; keeps A, X and Y
        bmi +
        sta LCD_DATA
        rts
+       sta SERIAL_DATA
        rts

puts    sta PTR                 ; print the string at A (low), Y (high)
        sty PTR+1
        ldy #0
-       lda (PTR),y
        beq +
        jsr putc
        iny
        bne -
+       rts

; ---- input, timing and random numbers ------------------------------------------

; Wait for a key and return it in A, in upper case.  While waiting, the
; random number generator keeps stepping, so the cards depend on timing.
getkey  lda FIXED
        bne +
        jsr next_random
+       lda SERIAL_STATUS
        bpl getkey
        lda SERIAL_DATA
        cmp #'a'
        bcc +
        cmp #'z'+1
        bcs +
        and #$DF
+       rts

; Wait about A thirds of a second at the default 1 MHz speed limit.
delay   sta STEPS
_outer  ldy #0
_mid    ldx #0
_inner  dex
        bne _inner
        dey
        bne _mid
        dec STEPS
        bne _outer
        rts

; Random number from 0 to A inclusive, in A.  Keeps X and Y.
random  sta LIMIT
        cmp #0
        beq _done
        lda #$FF                ; smallest all-ones mask covering LIMIT
_mask   sta MASK
        lsr a
        cmp LIMIT
        bcs _mask
_try    jsr next_random                 ; retry values above LIMIT
        and MASK
        cmp LIMIT
        beq _done
        bcs _try
_done   rts

; 16-bit xorshift generator (7, 9, 8) by John Metcalf.  Returns a random
; byte in A; RNG must never be zero.
next_random     lda RNG+1
        lsr a
        lda RNG
        ror a
        eor RNG+1
        sta RNG+1
        ror a
        eor RNG
        sta RNG
        eor RNG+1
        sta RNG+1
        rts

; ---- data --------------------------------------------------------------------
BASE    .byte $00, $10          ; offset of each hand in HANDS
ranks   .text "A23456789TJQK"
score_of .byte 1, 0, 2, 0, 1, 0 ; for each RES_*: 0 = win, 1 = loss, 2 = push

words   .word w_lose, w_win, w_push, w_bj, w_bust, w_win
w_lose  .null "LOSE"
w_win   .null " WIN"
w_push  .null "PUSH"
w_bj    .null " BJ!"
w_bust  .null "BUST"

messages .word m_lose, m_win, m_push, m_bj, m_bust, m_dbust
m_lose  .text "Dealer wins.", 10, 0
m_win   .text "You win!", 10, 0
m_push  .text "Push.", 10, 0
m_bj    .text "Blackjack! You win.", 10, 0
m_bust  .text "Bust! Dealer wins.", 10, 0
m_dbust .text "Dealer busts. You win!", 10, 0

title1  .null "   BLACKJACK"
title2  .null " PRESS ANY KEY"
intro   .text 10, "BLACKJACK", 10
        .text "Dealer's cards are on the top row of the LCD, yours below.", 10
        .text "H = hit, S = stand. The dealer stands on 17.", 10
        .text "Press any key to deal.", 10, 10, 0
prompt  .null "H)it or S)tand? "
s_d     .null "D:"
s_p     .null "P:"
s_dealer .null "Dealer: "
s_you   .null "You:    "
s_eq    .null " = "
s_wins  .null "Wins "
s_losses .null "  Losses "
s_pushes .null "  Pushes "
s_next  .text 10, "Press any key to deal.", 10, 10, 0
s_shuffle .text "(Shuffling the deck.)", 10, 0
