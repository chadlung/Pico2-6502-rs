; Memory-mapped I/O of the Pico 6502 computer (64tass syntax).

LCD_CONTROL   = $F000   ; W:  $01 clear display + home cursor, $02 home cursor
LCD_DATA      = $F001   ; W:  print character, advance cursor (CR, LF move it)
LCD_ROW       = $F002   ; RW: cursor row 0-1
LCD_COL       = $F003   ; RW: cursor column 0-15
LCD_BACKLIGHT = $F004   ; RW: 0 = off, anything else = on

SERIAL_DATA   = $F010   ; W: send byte over USB serial, R: next received byte
SERIAL_STATUS = $F011   ; R: bit 7 set when a received byte is waiting
