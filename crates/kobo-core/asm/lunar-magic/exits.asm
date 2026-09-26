; Screen exits and secondary entrances in Lunar Magic's format.
;
; Kobo's own code, written from the formats the community documents
; (docs/lunar-magic.md), the vanilla code it hooks, and where a Lunar
; Magic-saved ROM's entrance code sends each kind of exit
; (examples/exit_probe.rs), never from Lunar Magic's code.
;
; An exit's second byte is 0000wush: in Lunar Magic's format (u), h is the
; destination's bit 8, s makes it a secondary exit, and w makes the
; secondary entrance a water level; in the game's, bit 0 is kept and bit 1
; is the level's secondary flag. A secondary entrance's destination has its
; bit 8 in bit 3 of its $05FE00 byte (IPYXDAAA). An exit in the game's
; format keeps the game's rule, the destination's bit 8 from the player's
; submap.
;
; Lunar Magic's first save points $05D7CE at its own code and writes its
; own over the other sites, reading the same data.

lorom

; The screen exit object keeps the flags nibble for the screen, and the
; level's secondary flag is the exit's s bit.
org $0DA532 : db $0F            ; AND #$01
org $0DA536 : AND #$02          ; LDA $0B, before LSR : STA $1B93

; The destination's bit 8: BEQ : LDA #$01 after the submap's LDA.
org $05D7CE
    autoclean JSL exit_high

; A secondary entrance's action, and its destination's bit 8: AND #$07 :
; STA $192A, with A its $05FE00 byte.
org $05D836
    JSL entrance_type
    NOP

freecode

; A (8-bit) = the player's submap, X = the exit's screen. Returns A = the
; destination's bit 8; leaves the exit's flags in $02 for entrance_type.
exit_high:
    XBA                         ; the submap
    LDA $19D8,x
    BIT #$04
    BNE .lunar_magic
    STZ $02
    XBA                         ; the game's format: the submap
    BEQ +
    LDA #$01
+   RTL
.lunar_magic:
    STA $02
    AND #$02
    LSR A
    STA $1B93                   ; secondary, by this exit
    LDA $02
    AND #$01
    RTL

; A (8-bit) = the entrance's $05FE00 byte. Sets $0F to its destination's
; bit 8 and $192A to its action, with $40 for an exit's water bit.
entrance_type:
    PHA
    LSR A
    LSR A
    LSR A
    AND #$01
    STA $0F
    LDA $02
    AND #$08                    ; w, of an exit in Lunar Magic's format
    ASL A
    ASL A
    ASL A
    STA $02
    PLA
    AND #$07
    ORA $02
    STA $192A
    RTL
