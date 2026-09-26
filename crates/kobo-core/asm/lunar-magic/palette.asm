; Lunar Magic's custom level palettes.
;
; Kobo's own code, written from the format the community documents
; (docs/lunar-magic.md) and the vanilla code it hooks, and checked against
; the colours a Lunar Magic-saved ROM's levels load, never from Lunar
; Magic's code.
;
; A level's 3-byte pointer at $0EF600 leads to $202 bytes: the back area
; colour, then all 256 colours; $000000 or $FFFFFF for none. The hook
; replaces the JSL after the game assembles the level's palette in RAM
; (LoadPalette), in the level's setup (game mode $12), and copies the
; custom palette over it, before the game uploads it. It clears $00FE,
; which the level number hook sets to the level plus one, as a Lunar
; Magic-saved ROM's load leaves it wherever the setup runs this hook.

lorom

org $00A5BF
    autoclean JSL custom_palette    ; JSL CODE_05BE8A

freecode

custom_palette:
    PHP
    REP #$30
    LDA $010B                       ; the level (level.asm)
    ASL A
    CLC
    ADC $010B
    TAX
    LDA.l $0EF600,x
    STA $00
    LDA.l $0EF601,x
    STA $01
    CMP #$FFFF                      ; $FFFFxx: none
    BEQ .done
    ORA $00
    BEQ .done                       ; $000000: none
    LDX #$0000
-   TXY
    LDA [$00],y
    STA.l $7E0701,x                 ; BackgroundColor, then MainPalette
    INX
    INX
    CPX #$0202
    BNE -
.done:
    STZ $00FE                       ; as a Lunar Magic-saved ROM's load leaves it
    PLP
    JML $05BE8A                     ; what the hook replaces
