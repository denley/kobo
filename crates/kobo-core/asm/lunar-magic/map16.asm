; Map16 tiles past page 1, in Lunar Magic's layout.
;
; Kobo's own code, written from the community's description of the layout
; and the vanilla code it hooks (docs/lunar-magic-install.md), never from
; Lunar Magic's. Where Lunar Magic fixes an entry point, a JML sends it to
; Kobo's routine; the page tables sit at the fixed addresses Lunar Magic and
; the tools read them from, as data the routine reads.

lorom

; The per-tileset page 2 flag: off until a build writes such a page.
org $06F547
    db $00

; Entry points.
org $06F540
    autoclean JML map16_level
org $06F5D0
    JML map16_tile_change
org $06F5E4
    JML map16_overworld

; The row and column uploads of both layers: TAY : LDA Map16Pointers,Y.
org $058A65 : JSL $06F540
org $058B45 : JSL $06F540
org $058C33 : JSL $06F540
org $058D2A : JSL $06F540

; The stripe builders for a tile changed in play: REP #$20 : LDA
; Map16Pointers,Y. The byte after the JSL runs on return.
org $00C17A : JSL $06F5D0 : NOP
org $00C25C : JSL $06F5D0 : NOP

; The overworld's layer 1 tilemap build: ASL : ASL : ASL : TAY.
org $04DCFA : JSL $06F5E4

; A generated tile's page is set outright, not bit 0 of whatever page the
; tile it replaces was on: AND #$00 for page 0, LDA #$01 for page 1.
org $00C096 : AND #$00
org $00C0E7 : LDA #$01

freecode

; A (16-bit) = tile * 2, index registers 16-bit. Returns the address of the
; tile's 8-byte definition, the low word in A and the bank in $0C. X kept.
map16_level:
    JSR map16_find
    PHA
    SEP #$20
    TYA
    STA $0C
    REP #$20
    PLA
    RTL

; A 8-bit, Y (16-bit) = tile * 2. Returns with A 16-bit holding the
; address's low word, and its bank in $06, as the REP #$20 : LDA it
; replaces left them. X kept.
map16_tile_change:
    REP #$20
    TYA
    JSR map16_find
    PHA
    SEP #$20
    TYA
    STA $06
    REP #$20
    PLA
    RTL

; A (16-bit) = tile number. Y = tile * 8, the tile's index in the overworld's
; table at [$65], as the instructions it replaces leave it.
map16_overworld:
    ASL A
    ASL A
    ASL A
    TAY
    RTL

; A (16-bit) = tile * 2. Returns the definition's address, low word in A and
; bank in Y. X kept. Pages 0 and 1 come from the game's pointer table in RAM,
; bank $0D; pages 2 to $7F from the page tables.
map16_find:
    CMP #$0400
    BCS .custom
    TAY
    LDA $0FBE,y
    LDY #$000D
    RTS

.custom:
    LSR A
    AND #$7FFF
    PHX
    PHB
    PHA                       ; the tile, then its offset in its table
    PEA $0606                 ; the page tables are in bank $06
    PLB
    PLB
    XBA
    AND #$00F0
    LSR A
    LSR A
    LSR A
    TAX                       ; X = its group of 16 pages, times 2
    LDA 1,s
    SEC
    SBC.l .first,x
    ASL A
    ASL A
    ASL A
    STA 1,s
    CPX #$0000
    BNE .table
    CMP #$0800                ; page 2, with tables per tileset?
    BCS .table
    LDA $F547
    AND #$00FF
    BEQ .table
    LDA.l $001931             ; the object tileset
    AND #$000F
    XBA
    ASL A
    ASL A
    ASL A
    CLC
    ADC #$1000
    CLC
    ADC 1,s
    CLC
    ADC $F586
    STA 1,s
    LDA $F58A
    BRA .done

.table:
    LDA.l .pointer,x
    TAY
    LDA.w $0000,y             ; the table's address, from bank $06
    CLC
    ADC.l .adjust,x
    CLC
    ADC 1,s
    STA 1,s
    LDA.l .bank,x
    TAY
    LDA.w $0000,y
.done:
    AND #$00FF
    TAY
    PLA
    PLB
    PLX
    RTS

; Each group's first tile, and where its table's pointer and bank are.
; Pages $20-$3F and $60-$7F keep the pointer less one.
.first:
    dw $0200, $1000, $2000, $3000, $4000, $5000, $6000, $7000
.pointer:
    dw $F553, $F55C, $F567, $F570, $F594, $F59D, $F5A8, $F5B1
.bank:
    dw $F557, $F560, $F56B, $F574, $F598, $F5A1, $F5AC, $F5B5
.adjust:
    dw 0, 0, 1, 1, 0, 0, 1, 1
