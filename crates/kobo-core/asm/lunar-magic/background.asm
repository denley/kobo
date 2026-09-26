; Backgrounds in Lunar Magic's formats, and BG Map16 past the game's own.
;
; Kobo's own code, written from the community's description of the formats
; (docs/lunar-magic.md), the vanilla code it hooks, and what a Lunar
; Magic-saved ROM's level load leaves (examples/bg_survey.rs), never from
; Lunar Magic's code.
;
; A level's flags at $0EF310 (bbBBVFCT) say what its layer 2 pointer holds
; when its bank is not $FF: with C, Lunar Magic's own background, its tiles'
; high bytes in the stream too with F (32 rows to a half, 2048 bytes);
; with V, one in the game's format behind a full pointer; with neither,
; layer 2 objects. The high nibble is the high byte of a background's tiles
; when the stream has none, and for Lunar Magic's own format picks the BG
; Map16 table, one of 16 3-byte pointers at $0EFD50; every other background
; uses the first. The level load copies the flags to $7FC00B.
;
; Lunar Magic keeps the BG Map16 piece (the $058DA4 hook, the column upload
; changes, the pointers, and the routine's place at $0EFD00, which its own
; background code calls too) when that hook is a JSL, and the background
; piece, the flags with it, when the byte at $0EF519 is a JML: its layout
; has the background code at $0EF510, which its every save rewrites with
; its own. So $05803B jumps there, as in its layout, and Kobo's entry there
; has its JML at $0EF519. A save frees the block that JML leads to and
; writes its own code at $0EF510, so the background code is alone in its
; block.

lorom

!FLAGS = $7FC00B

; Per-level flags, none, and BG Map16 as the game has it, until a build
; writes its own.
org $0EF310
    fillbyte $00 : fill 512
org $0EFD50
    dl $0D9100
    fillbyte $00 : fill 45

; The background load: CMP #$FF : BNE on the layer 2 pointer's bank, with
; A 8-bit holding it and X and Y 16-bit.
org $05803B
    JML background_entry

org $0EF510
background_entry:               ; A 8-bit = the layer 2 pointer's bank
    PHA
    REP #$20
    LDA $010B                   ; the level (level.asm)
    TAX
    SEP #$20
    autoclean JML background    ; at $0EF519, which Lunar Magic's save checks

; The background column upload: the table and bank the level's flags
; pick, and bytes per background screen, $1B0 for 27 rows or $200 for 32.
org $058DA4
    JSL bg_map16_entry
    NOP

; Where Lunar Magic's layout has the table routine, which its background
; code (at $0EF510 after a save) also calls.
org $0EFD00
bg_map16_entry:
    autoclean JML bg_map16_table
org $058DB1 : ADC $05 : NOP     ; ADC #$01B0
org $058DB9 : ADC $05 : NOP     ; ADC #$01B0
org $058DCA : NOP #2            ; STY $0C: the hook sets the bank
org $058E12 : CMP $05 : NOP     ; CMP #$01B0

freecode

; X = the level, the layer 2 pointer's bank on the stack.
background:
    LDA.l $0EF310,x
    STA.l !FLAGS
    PLA
    CMP #$FF
    BNE .full
    JML $05803F                 ; the game's format in bank $0C

.full:
    LDA.l !FLAGS
    BIT #$0A
    BNE .background
    JML $058074                 ; layer 2 objects

.background:
    BIT #$04
    BNE .decode                 ; the stream has the high bytes
    LSR A                       ; else every tile's high byte is the
    LSR A                       ; flags' high nibble
    LSR A
    LSR A
    LDX #$0000
-   STA.l $7EBD00,x
    STA.l $7EBF00,x
    INX
    CPX #$0200
    BNE -
.decode:
    JML $058064                 ; the game's decode, into $7EB900 on

; In a block of its own: a save frees the block the JML at $0EF519 leads to,
; along with the code there, and the background code with it.
freecode

; Returns $0A-$0C = the level's table, $05-$06 = bytes per background
; screen, and A = $1928 in the caller's accumulator size: from the $058DA4
; hook, A 16-bit, as the instructions it replaces leave it. Lunar Magic's
; background code, after a save, calls it with A 8-bit.
bg_map16_table:
    PHP
    REP #$20
    SEP #$10
    LDA.l !FLAGS
    BIT #$0002
    BNE +
    LDA #$0000                  ; the game's format: the first table
+   AND #$00F0
    LSR A
    LSR A
    LSR A
    LSR A
    STA $0A
    ASL A
    CLC
    ADC $0A
    TAX                         ; the table's pointer, 3 bytes each
    LDA.l $0EFD50,x
    STA $0A
    SEP #$20
    LDA.l $0EFD52,x
    STA $0C
    REP #$20
    LDA.l !FLAGS
    AND #$0004
    BEQ +
    LDA #$0200
    BRA ++
+   LDA #$01B0
++  STA $05
    PLP
    LDA $1928
    RTL
