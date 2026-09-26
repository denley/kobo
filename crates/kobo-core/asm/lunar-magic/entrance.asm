; Lunar Magic's entrance settings: the per-level tables ($05DE00, $06FA00,
; $06FC00, $06FE00), the separate midway entrance, and a secondary
; entrance's two further tables, in the formats the community documents
; (docs/lunar-magic-install.md, "Entrances, exits, and midway points").
;
; Kobo's own code, written from those formats, the vanilla code it hooks,
; and what a Lunar Magic-saved ROM's entrance code leaves in RAM for every
; value of every settings byte (examples/entry_probe.rs), never from Lunar
; Magic's code. The JSL at $05DA17 is Lunar Magic's check for the
; per-level tables, and a save keeps them with it there; every save writes
; its own code over the entrance code's sites, reading the same tables.
;
; Needs exits.asm, which leaves a secondary entrance's number in $0BF6 and
; an exit's flags in $0BF8 ($0BF6-$0BF8 are the sprite graphics buffer,
; which the level's graphics fill only later).

lorom

; The entrance code's end, where every kind of entrance has been set up:
; SEP #$30 : LDA $13BF.
org $05DA17
    autoclean JSL entry_settings
    NOP

; The midway entrance's screen: LSR #4 of the $05F400 byte.
org $05D9E3
    JSL midway_screen

; The entrance action's setup, after the level's RAM is cleared: LDA $1C :
; CMP #$C0.
org $00A6CC
    JSL entry_flags

; The midway tape sets the midway point on any screen: BEQ over it when
; $13CD is 0.
org $00F2DB
    NOP #2

; The per-level tables as a fresh install has them; a build writes them.
org $05DE00
    fillbyte $00 : fill $200        ; IWPXXtTT
org $06FA00
    fillbyte $20 : fill $200        ; SHCvvvvv
org $06FC00
    fillbyte $00 : fill $200        ; OFYYYYYY
org $06FE00
    fillbyte $1A : fill $200        ; RL-ooooo

freedata

; Separate midway settings, one byte per level in each of four tables:
; IWHMXAAA, yyyyxxxx, RLE-ffbb, -FYYYYYY. A build writes them.
midway_tables:
    fillbyte $00 : fill $800

; A secondary entrance's two further bytes, EFYYYYYY and RLW-----, one per
; entrance in each table, behind the pointers where Lunar Magic keeps them.
; A build writes them.
freedata
entrance_table_5:
    fillbyte $00 : fill $200
freedata
entrance_table_6:
    fillbyte $00 : fill $200

org $05DC86
    autoclean dl entrance_table_5
org $05DC8B
    autoclean dl entrance_table_6

freecode
prot midway_tables

; A (8-bit) = the level's $05F400 byte, X and Y 8-bit. Returns A = the
; midway entrance's screen. The table address sits $0A bytes in, where the
; community's format documentation finds it.
midway_screen:
    LSR A
    LSR A
    LSR A
    LSR A
    REP #$10
    LDX $0E
    XBA
.table:
    LDA.l midway_tables,x
assert midway_screen_table == midway_screen+9
    STA $00
    LDA.l midway_tables+$400,x
    AND #$20
    BNE .redirect
    LDA $00
    AND #$10
    STA $00
    XBA
    ORA $00
    SEP #$10
    RTL
.redirect:
    ; The midway entrance of another level: start over with it.
    LDA.l midway_tables+$400,x
    AND #$01
    STA $0F
    LDA.l midway_tables+$200,x
    STA $0E
    SEP #$10
    PLA
    PLA
    PLA
    JML $05D8B7

; Returns with layer 1's position compared with the level's bottom, where
; the code it replaces compares $1C with $C0: vertical scrolling at will
; starts off only there.
entry_flags:
    LDA $192A
    BPL +
    LDA #$80
    STA $86                     ; slippery
+   BIT $192A
    BVC +
    LDA #$01
    STA $85                     ; water
+   LDA $192A
    AND #$3F
    STA $192A
    REP #$20
    LDA $5B
    LSR A
    LDA #$00C0
    BCC +
    LDA $5E                     ; $5F: the vertical level's screen count
    AND #$FF00
    SEC
    SBC #$0100
+   CMP $1C
    SEP #$20
    RTL

entry_settings:
    REP #$30
    LDA $0E
    AND #$01FF
    TAX
    SEP #$20
    LDA.l $05DE00,x
    STA $00
    AND #$04
    ASL A
    ASL A
    ASL A
    ASL A
    ASL A
    STA $02
    LDA $00
    AND #$03
    ORA $02
    STA $0BF4                   ; tTT
    LDA $1B93
    BEQ +
    JSR secondary
    BRA .done
+   LDA $13CF
    BNE .overworld_midway       ; from the overworld, past the midway point
    LDA $141A
    BEQ .main
    LDA $0BF8
    AND #$0A
    CMP #$08
    BNE .main                   ; an exit whose w, without s, leads here
    LDA.l midway_tables+$400,x
    AND #$20
    BNE .main                   ; which a redirected midway entrance ignores
.midway:
    LDA.l midway_tables,x
    AND #$20
    BEQ .main
    JSR midway
    BRA .done
.overworld_midway:
    LDA.l midway_tables,x
    AND #$20
    BNE .midway
    ; The game's midway entrance puts its screen in X's high byte; a
    ; vertical level takes it as Y's, and the layers start on it.
    LDA $5B
    LSR A
    BCC .main
    LDA $95
    STA $97
    STA $1D
    LDA $1414
    CMP #$03
    BEQ +
    LDA $95
    STA $21
+   STZ $95
.main:
    JSR main
.done:
    ; The game's vertical positioning sets the layer 1 vertical scroll
    ; setting early, which the camera then uses before the level's own
    ; replaces it; with the layers placed relative to the player it stays
    ; clear.
    LDA $5B
    LSR A
    BCC +
    LDA $13CD
    BPL +
    STZ $1412
+   SEP #$30
    LDA $13BF
    RTL

main:
    LDA $00
    AND #$C0
    TSB $192A
    LDA.l $06FE00,x
    STA $13CD
    LDA $00
    AND #$20
    BEQ .camera
    LDA.l $05F200,x
    AND #$07
    STA $02
    LDA $00
    AND #$18
    ORA $02
    STA $02                     ; the X tile, 5 bits
    LDA.l $05F000,x
    AND #$0F
    STA $03                     ; the Y tile's low 4 bits
    LDA.l $06FC00,x
    AND #$3F
    STA $04                     ; and its high 6
    JSR place
.camera:
    LDA $13CD
    BPL .done
    LDA.l $06FC00,x
    AND #$40
    LSR A
    LSR A
    STA $02
    LDA.l $05F400,x
    AND #$0F
    ORA $02
    STA $02                     ; Fffbb
    JSR relative_camera
.done:
    RTS

; A secondary entrance, whose number exits.asm left in $0BF6, with Lunar
; Magic's two tables of its own behind $05DC86 (EFYYYYYY) and $05DC8B
; (RLW-----), and its $05FE00 byte IPXXDAAA.
secondary:
    REP #$20
    LDA.l $05DC86
    STA $0A
    LDA $0BF6
    AND #$01FF
    TAY
    PHX
    TAX
    SEP #$20
    LDA.l $05DC88
    STA $0C
    LDA [$0A],y
    STA $01                     ; EFYYYYYY
    REP #$20
    LDA.l $05DC8B
    STA $0A
    SEP #$20
    LDA.l $05DC8D
    STA $0C
    LDA [$0A],y
    STA $00                     ; RLW-----
    AND #$20
    ASL A
    STA $02
    LDA.l $05FE00,x
    AND #$80
    ORA $02
    TSB $192A                   ; water and slippery
    LDA.l $05FE00,x
    AND #$40
    BEQ .camera
    LDA.l $05FE00,x
    AND #$30
    LSR A
    STA $02
    LDA.l $05FC00,x
    LSR A
    LSR A
    LSR A
    LSR A
    LSR A
    ORA $02
    STA $02                     ; the X tile, 5 bits
    LDA.l $05FA00,x
    AND #$0F
    STA $03
    LDA $01
    AND #$3F
    STA $04
    JSR place
.camera:
    LDA.l $05FA00,x
    LSR A
    LSR A
    LSR A
    LSR A
    STA $02
    LDA $01
    AND #$40
    LSR A
    LSR A
    ORA $02
    STA $02                     ; Fbbff
    PLX
    LDA.l $06FE00,x
    AND #$3F
    STA $03
    LDA $00
    AND #$C0
    ORA $03
    STA $13CD
    BPL +
    JMP relative_camera
+   RTS

midway:
    LDA.l midway_tables,x
    STA $00
    AND #$C7
    STA $192A                   ; the action, slippery and water
    LDA.l midway_tables+$400,x
    AND #$C0
    STA $02
    LDA.l $06FE00,x
    AND #$3F
    ORA $02
    STA $13CD
    LDA.l midway_tables+$200,x
    AND #$0F
    STA $02
    LDA $00
    AND #$08
    ASL A
    ORA $02
    STA $02                     ; the X tile, 5 bits
    LDA.l midway_tables+$200,x
    LSR A
    LSR A
    LSR A
    LSR A
    STA $03
    LDA.l midway_tables+$600,x
    AND #$3F
    STA $04
    LDA.l $05F400,x
    LSR A
    LSR A
    LSR A
    LSR A
    STA $95
    LDA $00
    AND #$10
    TSB $95                     ; the screen, 5 bits
    LDA $5B
    LSR A
    BCC +
    LDA $95                     ; a vertical level's screen is Y's
    STA $97
+   JSR place
    LDA $13CD
    BPL .fixed
    LDA.l midway_tables+$600,x
    AND #$40
    LSR A
    LSR A
    STA $02
    LDA.l midway_tables+$400,x
    AND #$0F
    ORA $02
    STA $02
    JMP relative_camera
.fixed:
    ; The game's initial positions, by ff and bb.
    REP #$20
    LDA.l midway_tables+$400,x
    PHX
    PHA
    AND #$000C
    LSR A
    LSR A
    TAX
    SEP #$20
    LDA.l $05D708,x
    STA $1C
    REP #$20
    PLA
    AND #$0003
    TAX
    SEP #$20
    LDA.l $05D70C,x
    STA $20
    PLX
    RTS

; The player's position from an entrance's tile: $02 the X tile (5 bits),
; $03 and $04 the Y tile's low 4 and high 6 bits. A horizontal level keeps
; the screen in $95 and takes 4 bits of X; a vertical one keeps it in $97
; and takes the Y tile's low 4 bits only.
place:
    LDA $5B
    LSR A
    BCS .vertical
    LDA $02
    ASL A
    ASL A
    ASL A
    ASL A
    STA $94
    REP #$20
    LDA $04
    AND #$003F
    ASL A
    ASL A
    ASL A
    ASL A
    STA $06
    LDA $03
    AND #$000F
    ORA $06
    ASL A
    ASL A
    ASL A
    ASL A
    STA $96
    SEP #$20
    RTS
.vertical:
    REP #$20
    LDA $02
    AND #$001F
    ASL A
    ASL A
    ASL A
    ASL A
    STA $94
    SEP #$20
    LDA $03
    ASL A
    ASL A
    ASL A
    ASL A
    STA $96
    RTS

; The layers' positions from the player's, for an entrance that sets them
; relative to the player: $02 the offset in rows (5 bits, signed), X the
; level. The background follows the level's settings: its height, or an
; offset from the foreground.
relative_camera:
    REP #$20
    LDA $1414
    AND #$00FF
    STA $05                     ; the vertical scroll setting, for Y
    LDA $02
    AND #$001F
    EOR #$0010
    SEC
    SBC #$0010
    ASL A
    ASL A
    ASL A
    ASL A
    CLC
    ADC $96
    BPL +
    LDA #$0000
+   STA $1C
    LDA.l $06FC00,x
    BIT #$0080
    BEQ .height
    LDA.l $06FE00,x
    AND #$001F
    CMP #$0010
    BEQ .absolute
    EOR #$0010
    SEC
    SBC #$0010
    ASL A
    ASL A
    ASL A
    ASL A
    CLC
    ADC $1C
    STA $20
    SEC
    SBC $1C
    LDY $05
    BEQ .offset
    BRA .scrolled
.absolute:
    STZ $20
    LDA #$0000
    SEC
    SBC $1C
    LDY $05
    BEQ .offset
    BRA .scrolled
.height:
    ; The background's bottom row where the level's is: the level's bottom
    ; camera position is $C0, or in a vertical level the top of its last
    ; screen; the background's is its height less $F0.
    LDA $5B
    LSR A
    LDA #$00C0
    BCC +
    LDA $5E                     ; $5F: the vertical level's screen count
    AND #$FF00
    SEC
    SBC #$0100
+   STA $02
    LDA $1C
    SEC
    SBC $02
    LDY $05
    BEQ .fixed
    DEY
    BEQ .rated
    CMP #$8000
    ROR A
    DEY
    BEQ .rated
    CMP #$8000
    ROR A
    CMP #$8000
    ROR A
    CMP #$8000
    ROR A
    CMP #$8000
    ROR A
    BRA .rated
.fixed:
    LDA #$0000
.rated:
    STA $02
    LDA.l $06FE00,x
    AND #$001F
    ASL A
    ASL A
    ASL A
    ASL A
    SEC
    SBC #$00E0
    CLC
    ADC $02
    STA $20
    LDY $05
    BEQ .done
.scrolled:
    ; The layer 2 offset the camera keeps from here on.
    LDA $1C
    DEY
    BEQ +
    LSR A
    DEY
    BEQ +
    LSR A
    LSR A
    LSR A
    LSR A
+   EOR #$FFFF
    SEC
    ADC $20
.offset:
    STA $1417
.done:
    SEP #$20
    RTS
