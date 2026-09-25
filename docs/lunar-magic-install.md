# Lunar Magic's one-time install

What Lunar Magic 3.70 writes only on the first save into a ROM whose gate `$06F600` is
still `$FF`, what each piece replaces in the vanilla game, what it is for, and what it has
to leave behind. A Kobo build that sets the gate gets none of it from Lunar Magic, so Kobo
provides all of it ([step-2.md](step-2.md), work order 2b). The rest of Lunar Magic's
footprint, and the spike that found this set, are in [lunar-magic.md](lunar-magic.md);
the vanilla code named here is described in [smw.md](smw.md).

Confidence, per statement: *documented* (Lunar Magic's help, the community's format
documentation, or a tool's source), *observed* (a byte diff's addresses, data tables'
values, or RAM after a level load), *inferred* (from the vanilla code a change covers and
the feature it belongs to). Nothing here comes from reading Lunar Magic's code: every
change to vanilla code is described by the vanilla instructions it covers and the effect
seen, never by what Lunar Magic wrote.

## How it was found

- The set is the spike's: `e0` is the vanilla ROM after `-ImportLevel` of level `105`'s own
  MWL; `e9` is `e0` with every changed range outside the 52 hook sites put back to vanilla
  and the gate left set, saved again. What that save left vanilla is the one-time set: 15
  hooks and 95 ranges (`$695` bytes). Ranges it restored only in part are split here into
  their one-time and restored parts.
- `tools/lunar-magic/sites.py` names the vanilla instructions or data a range covers
  (SMWDisX assembled with `--symbols=wla`, whose addr-to-line map ties each address to a
  source line) and which of their byte offsets changed. Of Lunar Magic's bytes it prints
  only the target of a jump at a hook site.
- `tools/lunar-magic/ramdiff.py` loads the same levels in two ROMs with `kobo level wram`
  and reports the RAM that differs (the stack blanked, since it holds return addresses);
  `--revert` puts chosen ranges of the second ROM back to the first's bytes, so a hook's
  effect shows as what disappears without it. Levels used: `105` (mode `00`, background),
  `1A` (`02`, layer 2 objects), `E` (`01`), `1CE` (`08`, vertical), `C2` (`0A`, vertical
  with background), `95` (boss), `4` (`0C`), `18` (`0E`), and all 512 for the summary.
- Every ROM of `KOBO_LM_ROMS` (Lunar Magic 1.62 to 3.51) was compared with `e0` range by
  range, which dates some pieces: what matches `e0` only in 3.x ROMs came with 3.00, what
  no corpus ROM has is newer than 3.51.
- Accessible documentation: the help file's technical pages (`info_map16_gameplay`,
  `info_screen_exit_routine`, `info_decompress`, `info_command_line`) and its version
  history (`changes`), the smwspeedruns level format page (saved in
  `~/.local/share/kobo/docs/`), and the sources of PIXI, GPS, UberASM Tool, AddmusicK, and
  SA-1 Pack (not `boost/lz3.asm`).

## What a save does with the gate set

Found by saving copies of `e0` with parts of the set changed (level `105` re-imported each
time); the command line reports success in every case.

- Foreign bytes in every one-time code area (`$00BA56`, `$03BCDC`, `$05DD00`, `$06F540`-
  `$06F5FF`, the acts-like code from `$06F65C`, `$0DE1D0`-`$0DE24F`, `$0EFD00`-`$0EFD3F`)
  survive a save, alone or with every one-time change to vanilla code put back to vanilla.
  Nothing of the set is reinstalled. Kobo's code at Lunar Magic's fixed addresses is kept.
- Some areas outside the set are Lunar Magic's on every save: it writes its own code over
  whatever is at `$03BB00`-`$03BB1E`, `$03BCA0`-`$03BCBF`, `$05DD30`-`$05DD7F`, and
  `$0EF510`-`$0EF54F`, and its table pointers into `$05DC81`, `$05DC86`, `$05DC8B`,
  `$0DE191`, `$0DE198`, and `$0DE19F` (the rest of `$05DC50`-`$05DC8E` and
  `$0DE190`-`$0DE1CF`, `$03BCE0`-`$03BD9F`, `$05DCB0`, and `$05DCCC` keep foreign bytes and
  are rewritten only from `$FF`). Two one-time hooks jump into those areas (`$05803B` to
  `$0EF510`, `$05DBC2` to `$03BB00`), so after a save they run Lunar Magic's code whatever
  Kobo put there. Kobo's two hooks can instead jump to Kobo's own blocks, since Lunar
  Magic keeps retargeted hooks; the four areas must then stay free for Lunar Magic.
- `$0FF035` is rewritten by every save, to a value that depends on the state of the
  one-time code (`$D8` after a fresh install; `$00`, `$7F`, or `$FF` with single pieces
  reverted). When the bytes below it are not `$FF` it also rewrites some of them (with all
  one-time code foreign, `$0FEFC8`-`$0FF034`; over AddmusicK's `$55` padding,
  `$0FEF9F`-`$0FF091`). What the byte records is unknown. Most corpus ROMs have `$00`
  there and non-`$FF` bytes below, several `$FF` and nothing below.
- Resaving a 3.31 ROM (`SMW_2022-4-9`) with 3.70 changed 11 items of the set: it added the
  3.50 Choc Island 2 hook and the 3.70 game loop hook and rewrote `$0085D2`,
  `$00A2AF`, and `$0580C0`, so 3.70 upgrades an older install in place. It does not put
  the same pieces back into a 3.70 install they were removed from, whether the marker at
  `$0FF0A0` says 3.70, 3.30, or is absent. What decides the upgrade is unknown; a foreign
  install was not upgraded.

## The hooks

Each is a `JSL` over the vanilla instructions named; bytes after the four of the `JSL` are
filler. "Target" areas are Lunar Magic's fixed code.

| Site | Vanilla (SMWDisX) | Target | Feature | Contract | Confidence |
|---|---|---|---|---|---|
| `$058A65`, `$058B45`, `$058C33`, `$058D2A` | `TAY : LDA Map16Pointers,Y` in the layer 1 and 2 row and column uploads (`CODE_0589xx`-`058Dxx`) | `$06F540` | Map16 pages 2-`7F` | A (16-bit) = tile*2 in; out: A = definition address low word, `$0C` = its bank, Y free; `[$0A]` then reads the 8 bytes | documented (lunar-magic.md), used by `expand` |
| `$00C17A`, `$00C25C` | `REP #$20 : LDA Map16Pointers,Y` in the tile-change stripe builders (`CODE_00C13E`, `CODE_00C222`) | `$06F5D0` | the same, for a tile changed in play | Y = tile*2 in; out: A (16-bit) the address low word, `$06` its bank (vanilla sets `$06 = $0D` just before) | inferred |
| `$04DCFA` | `ASL : ASL : ASL : TAY` in `CODE_04DCB6`, the overworld layer 1 tilemap build | `$06F5E4` | overworld layer 1 16x16 tiles past page 0 (1.90: two pages) | A (16-bit) = tile from `$7EC800`/`$7FC800` in; out: Y indexes `[$65]` for the tile's 4 words | inferred; not observable at level load |
| `$02BA9E` | `INC $07 : LDA [$05]` (tile high byte) in `CODE_02B9FA`, Yoshi's berry check | `$06F820` | acts-like for Yoshi's tongue and berries (3.70, "Yoshi's tongue touched a block") | out: A = high byte of the tile to act as; `$1693` = its low byte | inferred; 3.70 only (vanilla in all 47 corpus ROMs) |
| `$05803B` | `CMP #$FF : BNE` on the layer 2 pointer's bank in `CODE_05801E` | `$0EF510` (Lunar Magic's on every save) | per-level background flags and formats | `$7FC00B` = the level's `$0EF310` byte; background decoded to `$7EB900` (low) and `$7EBD00` (high); `$7FBC00`/`$7FC300` filled (below) | observed by ablation |
| `$058DA4` | `STA $0A` of `Map16BGTiles` in the background column upload | `$0EFD00` | background Map16 from `$0EFD50` | `$0A`-`$0C` = the level's BG Map16 table; `$05`-`$06` = bytes per background screen (`$01B0`, `$0200` for 32 rows) | documented (lunar-magic.md), observed |
| `$05D7CE` | `BEQ : LDA #$01` choosing the destination's high byte in the screen exit path | `$05DC50` | exits to any level; secondary exits by `$19D8` | reads `$19D8,X` (`$04` Lunar Magic format, bit 0 destination bit 8, bit 1 secondary, bit 3 water, copied to `$192A`) | documented (smw.md, UberASM Tool's and GPS's teleport routines store the high byte `ORA #$04` there) |
| `$05D8E2` | `LDA $0E : ASL : TAY` before the sprite pointer read | `$0EF550` | full level number for code | out: Y = level*2 (16-bit); `$010B` = level (16-bit; PIXI's own `$05D8B9` hook stores the same); `$00FE` = level + 1 | observed by ablation |
| `$05DB5B` | `STA $68 : SEP #$20 : LDA [$CE] : AND #$7F` in `CODE_05DB3E`, Choc Island 2's rooms | `$0DE210` | 24-bit pointers to Choc Island 2's extra rooms (3.50) | the rooms' layer 1, layer 2, and sprite pointers with banks; `$1692` sprite memory from the header as the main path masks it | documented (changes 3.50), inferred; 3.50+ only |
| `$05DBC2` | `STA $19B8,X : INC $141A` in `CODE_05DBAC`, the bonus game and Yoshi wings exit | `$03BB00` (Lunar Magic's on every save) | the bonus room exit in the screen-exit format; 3.60 fixed it for low screens of tall levels | `$19B8,X` and `$19D8,X` for the screen Mario is on (`$03BCDC`), `$141A` incremented | documented (changes 1.42, 3.60), inferred |
| `$04E5F1` | `CMP #$02 : BNE : INC $1DEA` in `CODE_04E5EE` (secret exit adds 1 to the event) | `$05DCB0` | Secret Exit 2 and 3 (3.00), exits to the overworld from secondary entrances | the event number `$1DEA` for exit modes 2-4 | documented (changes 3.00), inferred |

## The ranges

### Map16 pages and the acts-like chain (bank `$06`)

- `$06F540`-`$06F643`, all vanilla `$FF` fill: the Map16 routine (`$06F540`) and its two
  other entries (`$06F5D0`, `$06F5E4`), the gate, and the acts-like pointers.
  - Fixed operands others read (documented, smwspeedruns): page tables at
    `$06F553`/`$06F557` (pages `02`-`0F`, 16-bit address and bank), `$06F55C`/`$06F560`
    (`10`-`1F`), `$06F567`/`$06F56B` (`20`-`2F`, address + 1), `$06F570`/`$06F574`
    (`30`-`3F`, + 1), `$06F594`/`$06F598`, `$06F59D`/`$06F5A1`, `$06F5A8`/`$06F5AC`,
    `$06F5B1`/`$06F5B5` (`40`-`7F` likewise); `$06F547` non-zero turns on per-tileset
    page 2, whose table is `read3` at `$06F586`/`$06F58A` plus `$1000`, `$800` bytes per
    tileset. Kobo's routine has to hold these values at exactly these addresses, which
    fixes much of its layout.
  - `$06F600`: the gate, any byte but `$FF` (3.x writes `$EA`, 1.62 and 2.41 `$68`). It is
    an instruction in Lunar Magic's code, so whatever Kobo puts there sits in its own code
    path. `$06F602` is the acts-like chain's common exit (GPS jumps to it).
  - `$06F624`: 3-byte pointer to the acts-like table, pages `00`-`3F`, 2 bytes per tile;
    PIXI and GPS refuse `$FFFFFF`. `$06F63A`: pointer, minus `$8000`, to pages `40`-`7F`;
    `$06F63C = $FF` means none (GPS). A fresh install points `$06F624` at a RATS block and
    leaves `$06F63A` with bank `$FF`. Chains resolve until a tile below `$200`.
- The acts-like entries (documented, help file): four calls to `RemapBlocks` are
  retargeted, `$00F4DD` (Mario) to `$06F660`, `$019533` (sprites) to `$06F700`,
  `$02961A` (cape) to `$06F760`, `$02A6EB` (fireballs, `CODE_02A611`) to `$06F7A0`; Yoshi's
  `JSL CODE_02B9FA` at `$01A24C` to `$06F845` and at `$01F589` to `$06F840`, and the
  berry's `JSL InitSpriteTables` at `$02BAE8` to `$06F8B0`. Code: `$06F65C`-`$06F67D`,
  `$06F690`-`$06F769`, `$06F780`-`$06F7B3`, `$06F7C0`-`$06F7CF`, `$06F7F0`-`$06F82D`,
  `$06F840`-`$06F8A0`, `$06F8B0`-`$06F8D8` (the last three, and `$02BA9E`, are 3.70's).
  Contract at the retargeted calls: vanilla `RemapBlocks`' (in: A = tile high byte,
  `$1693` = low byte; out: A = the high byte to report, 8-bit, `$1693` the low byte),
  with the tile resolved through the acts-like table first and, for each kind of contact,
  the custom block slots run.
  - Custom block slots (help file): up to three 4-byte `JSL`s at each of `$06F890`
    (Mario below), `$06F8A0` (above), `$06F8B0` (side), `$06F8C0` (top corner),
    `$06F8D0` (body), `$06F8E0` (head), `$06F920` (sprite above/below), `$06F930`
    (sprite side), `$06F980` (cape), `$06F9C0` (fireball), `$06F9F0` (Yoshi's tongue);
    `$06F8F0`, `$06F940`, `$06F950`, `$06F990` reserved. Called with A/X/Y 8-bit, X and Y
    to be preserved, Y = reported high byte and `$1693` the low byte (below `$200`), `$03`
    (16-bit) the last tile of the acts-like chain. `$1933`, `$185E`, `$0F` say which layer.
    After a fresh install `$06F890`-`$06F8A0` and `$06F8B0`-`$06F8D8` have changed and
    `$06F8A1`-`$06F8AF` and `$06F8D9`-`$06F9FF` are still `$FF`, and `$06F8B0`, the side
    slot, is also where the berry's retargeted `JSL` at `$02BAE8` goes. How that fits the
    documented slots, how Lunar Magic's code tells an empty slot, and what a block tool
    checks before writing one are unknown.
  - GPS (source, `main.asm`) writes 16-byte entries at `$06F690`, `$06F6A0`, `$06F6B0`,
    `$06F6C0`, `$06F6D0`, `$06F6E0`, `$06F720`, `$06F730`, `$06F780`, `$06F7C0`, `$06F7D0`,
    `$06F7E0` (below, above, side, top corner, body, head, sprite vertical, sprite
    horizontal, cape, fireball, wall feet, wall body): `PHB : PHX : REP #$30 : LDA #id*3+1
    : JSL : PLX : PLB : JMP $F602`. So each entry is entered with the data bank and X free
    to push, A/X/Y 8-bit, `$03` holding the tile (it does `LDX $03` and `BIT $03` for pages
    `40`+), and leaves through `$06F602`. It replaces 4 bytes at `$06F67B` with a `JML` whose
    code compares A with `#$39` and `#$EA` (the low byte of the caller's return address,
    for the wall-run calls) and at `$06F717` with `#$82` (sprite horizontal), falling back
    to `$06F602`: at those two addresses Kobo's chain must hold a 4-byte instruction slot
    reached with A = the caller's return address low byte, where falling through means "no
    custom block". GPS treats `$8B` at `$06F690` without its version string as foreign
    block code, so Kobo's entry there must not start with `$8B`.
- Tile generation (`GenerateTile`, `CODE_00C077`/`CODE_00C0C4`): the operand of
  `AND #$FE` at `$00C096` and the opcode of `ORA #$01` at `$00C0E7`, which set the new
  tile's high byte from what was there. With tiles on pages 2 and up, the page has to be
  set outright (0 or 1) rather than bit 0 toggled (inferred). In every corpus ROM from
  1.62 on.

### Custom block actions, observed

What Lunar Magic 3.70's chain runs as the player meets a custom block, found by playing a
Lunar Magic-saved vanilla ROM with a GPS probe block at tile `$200` that logs each action
it is called for with the game's touch position (`$98`-`$9B`) and the player's (`$94`-`$97`)
(`tools/lunar-magic/block-probe`, `examples/contact_probe.rs`; memory effects only). The
touch offset from the player names the interaction point through the game's hitbox tables
(`PlayerXHitboxPoints` `$00E830`, `PlayerYHitboxPoints` `$00E89C`), and the point names the
call site of `$00F44D` whose return address the chain sees (the low byte GPS compares):

| Interaction point (`NormalCollision`) | Call site, return low byte | Offset (small; big) | Action |
|---|---|---|---|
| 0, centre | `$00EBAF`, `$B1` | (8, 24); (8, 18) | body |
| 1, side body | `$00EC24`, `$26` | (14 or 2, 26) | side |
| 2, side head | `$00EC3A`, `$3C` | (14 or 2, 22); (14 or 2, 15) | head |
| 3, head | `$00EC8A`, `$8C` | (8, 16); (8, 8) | below |
| 4, right foot | `$00ED4A`, `$4C` | (11, 32) | above, or top corner |
| 5, left foot | `$00EDE9`, `$EB` | (5, 32) | above, or top corner |
| wall run | `$00EB37`, `$39` | | wall feet (GPS's own check) |
| wall run | `$00EFE8`, `$EA` | | wall body (GPS's own check) |

- A foot point gives "top corner" rather than "above" in some frames: with the left foot
  alone on the block (the right one over air) standing still, and for the foot still on
  the block while walking off either edge; with the right foot alone on the block
  standing still it gives "above". The condition is not yet known.
- The "head" action is the side head point, not the head point (which gives "below").
- Sprites, the cape, fireballs, and Yoshi's tongue are not yet probed.

### Taller levels (3.00, "ExLevel")

Vanilla finds a horizontal level's screens through fixed tables: `LoadBlkPtrs` (`$00BEA8`)
points per level mode at 3-byte per-screen pointer tables (`Ptrs00BDA8`, `Ptrs00BDE8`,
`Ptrs00BE28`, `Ptrs00BE68`: layer 1 and 2, low and high byte planes), and block
lookups add the split screen offsets `DATA_00BA60`/`BA70` (low) and `BA9C`/`BAAC` (high).
All assume 27 rows (`$1B0` bytes a screen).

- The 9 ranges `$00BDA8`-`$00BEA7`: in all four tables the entries for modes `00`, `01`,
  `02`, `0C`, `0E`, `0F`, `11`, `1E`, `1F` point at RAM instead: `$0BF6`, `$0C26`,
  `$0C56`, `$0C86` (observed; data).
- Lookups of `DATA_00BA60`/`BA70`/`BA9C`/`BAAC` have their operands changed at
  `$00F492`, `$00F49A`, `$00F50D`, `$00F515` (Mario), `$019500`-`$01951B` (sprites),
  `$01D97B`/`$01D981`, `$0292F9`-`$029312`, `$0295EC`-`$029605` (cape), `$02A6BA`-
  `$02A6D3` (fireballs), `$02BA71`-`$02BA8A` (Yoshi), `$02D18C`-`$02D1A5`; the bank stays
  `$00`, consistent with the RAM tables at `$0CB6`/`$0CD6`.
- Height bounds: `CMP #$01B0` at `$00F478`, `$00F4F3` (player block collision),
  `$0194D6` (sprites); `ADC #$01B0` at `$03D793`; the high byte of `AND #$01F0` (Y to
  row) at `$00C07B`, `$00C0C8`, `$00C1B3`, `$00C3D5`, `$058A18`, `$058AF4`, `$058BE6`,
  `$058CD9`; the branches after the on-screen checks in `CODE_00C0FB` (`$00C116`,
  `$00C122`); the VRAM address build in `GenerateTile` (`$00BF35`-`$00BF48`,
  `$00BF81`-`$00BF9A`). Inferred: heights past `$1F0` and the 64x32 tilemap of the VRAM
  patch.
- The object loader's screen stepping: `$0586A1`-`$0586B1` (Map16 pointers of a screen
  from `LoadBlkPtrs`), `$0DA963`-`$0DA973` and `CODE_0DA9D6`/`CODE_0DA9EF`
  (`$0DA9D6`-`$0DAA04`: `$1B0` added to or taken from `$6B`/`$6E`). Inferred: a stride
  of the level's rows*16.
- Sprites below the level: the `Y + $50 >= $200` erase test at `$01AC40`, `$02D03A`,
  `$02FED6` (cluster sprites), `$03B86C` now calls a routine (a `JSL` to a Lunar Magic
  block in `e0`); the goal tape keeps its Y high byte whole (`$01C08C`-`$01C093`,
  `$01C0E1`) while still saving the extra bits in `$187B` (Secret Exits 2 and 3 use
  them). PIXI rewrites `$01C089`-`$01C093` itself when `!EXLEVEL`: `$187B,X` = extra
  bits, `$14D4,X` = the Y high byte it stored.
- Extended objects (`$0DA10F` table, data): `01` (screen jump) re-pointed to `$0DE1D0`
  and `03` to `$0DE1E0`, both one-time code (`$0DE1D0`-`$0DE1FF`); `02` to `$0DE1B0`,
  which saves restore. Formats in [lunar-magic.md](lunar-magic.md) (screen jump's
  vertical part, 13-bit screen exit, mode `1C` jump). `$0DE1F0` is also the target of the
  restorable hook at `$0583C7`.
- Screen shake: `GrndShakeDispYLo`/`Hi` (`$00A1CE`-`$00A1D5`) become one table of
  16-bit words with the same offsets (-2, 0, 2, 0), and the code at `$00A2AF`-`$00A2D4`
  that applies them to `$1C` and `$1888` is rewritten (3.x ROMs differ from 3.70's).
  Inferred: a 16-bit add for Y positions past `$FF`, with room freed; the effect was not
  observed, since no shake happens during a load.
- `Layer1Map16DMAData` (data): the two column entries' sizes (`$008A3E`, `$008A4C`) go
  from `$2C` to `$40` bytes, 22 rows to 32 (inferred: the VRAM patch's 64x32 tilemap).
  As 3.70 in 45 of the 47 corpus ROMs.

### Backgrounds and the level load's uploads

- `$058DB1`-`$058DBB` (`ADC #$01B0`, twice), `$058DCA` (`STY $0C` of the BG Map16 bank),
  `$058E12` (`CMP #$01B0`) in the background column upload: the stride and bank come from
  what the `$058DA4` hook left (`$05`, `$0C`).
- `$0EFD00`-`$0EFD33`, `$0EFD3C`-`$0EFD3F`: the `$058DA4` hook's code. `$0EFD50`-
  `$0EFD7F`: 16 3-byte pointers, 16 BG Map16 pages each; a fresh install sets the first
  to `$0D9100` (vanilla's) and the rest to `$000000` (observed; documented).
- The initial tilemap upload: in `CODE_0580BD` the three `JSL`s (`$0580BF`, `$0580C3`,
  `$0580C7`) are retargeted to Lunar Magic blocks, and `LDA $47` at `$0580D3` jumps to
  `$0580FB`, past the vertical-pipe re-pointing (smw.md); `$05879D`-`$0587A1` does the
  same in the scroll setup `CODE_05877E` (inferred). The NMI's `JSL UploadOneMap16Strip`
  at `$008209` is retargeted too. Observed: without the `$0580BF` retargets the load
  leaves the vanilla state in `$1BE6`-`$1DE7` (the layer VRAM buffers), `$0695`-`$06B6`
  (VRAM addresses and buffer pointers, inside `DynPaletteTable`), `$7F819F`, and nothing
  in `$7FBC00`/`$7FC300`.

### Entrances, exits, and midway points

- Secondary entrances (data): exits `0CE`, `0CF`, `0D0`, `0D9`, `0DB`, `0DE`, `0E1`,
  `0F0`, `0F6`, `0F7`, `0FF`, which vanilla leaves with destination level 0 and stray
  position bytes, are cleared in all four tables (`$05FACE`-`$05FEF7`, 9 ranges); bit 3
  of `$05FE00` is set for exits `100`-`1FF` (`$05FF00`-`$05FFFF`), the destination's bit 8
  ("D" in the format), which vanilla leaves implicit in the submap. The save rewrote
  entry `1CB` of that table as well. Contract: Lunar Magic's exit code reads the
  destination from `$05F800` plus this bit.
- Midway points: the opcode of `STA $13CD` at `$05D9C3`, and the operand of `STA $95` and
  the `JMP CODE_05DA17` after it at `$05D9E7`, in the entrance code, and `BEQ` at
  `$00F2DB` in the midway tape's block code, which vanilla uses to skip recording a
  midway point on screen 0 (as 3.70 in 41 corpus ROMs). Documented
  (changes 2.20): separate midway coordinates, screen exits to midway entrances, the
  screen-0 fix. Observed: `$13CD` after a sublevel load is `$1A` in 27-row levels and
  other values (`$93`, `$9A`, `$D3`, `$DA`) in taller ones; vanilla leaves 0 there on that
  path. Its meaning is unknown.
- `DATA_05D710`/`DATA_05D720` (layer 2 vertical and horizontal scroll by the high nibble
  of `$05F000`; data): entries 8-11 become vertical settings 4-7 with horizontal 2.
  Lunar Magic's added layer 2 scroll speeds (3.40) are handled in its scroll code.
- `$05DD00`-`$05DD1C`: code, the target of the restorable hook at `$00A6CC` (the entrance
  setup after `CODE_00A6CC`, which checks `$1C == $C0` for vertical scrolling).
- `$03BCDC`-`$03BCDF`: the first four bytes of the documented screen-number routine
  (`JSL $03BCDC`: 8-bit in and out, X = the screen Mario is on, A and Y and `$00` (16-bit)
  clobbered); a save restores the rest. UberASM Tool's and GPS's teleport routines call it
  when `!EXLEVEL`.

### Sprites

- The loader loop (`$02A826`-`$02A83B`, from `BMI Return02A84B` to the first `INY`) now
  starts with a `JML` to a Lunar Magic block, and `INY : LDX $02` at `$02A9D7` becomes
  `JMP $A838`; `LDA $00` at `$02A968` changes opcode. Feature: the new sprite system's
  `$FF` commands and Y jumps (3.00). Contract with the tools: PIXI's and SA-1 Pack's
  loaders return to `$02A82E` as the loop head with Y at the next entry and X counted
  (`JML $02A82E`), PIXI's `SubLoadHack` returns to `$02A968`, and PIXI jumps from
  `$02A9DB` and `$02ABEF` on LoROM, so all four must stay entry points of Kobo's loop.
- `$02ABF3`, the operand of `LDX #$3F` in `CODE_02ABF2`: vanilla clears only 64 of the
  128 load flags. PIXI writes `$7F` there itself unless `!EXLEVEL` ("be able to load 128
  sprites"), so with a Lunar Magic 3 layout Kobo has to clear all 128.

### Per-level tables

Initialised for all 512 levels; a save rewrites the saved level's entry (observed, data).

- `$05DE00`-`$05DFFF`: all `$00` (fifth secondary header byte, `IWPXXtTT`).
- `$06FC00`-`$06FDFF`: all `$00` (`OFYYYYYY`); `$06FE00`-`$06FFFF`: all `$1A` (`RL-ooooo`,
  background height 27). `$06FA00` (`SHCvvvvv`, all `$20`: auto screen count) is written by
  every save, not once.
- `$0EF100`-`$0EF2FF`: sprite data banks, all `$07`. `$0EF300`-`$0EF30B`: code, the
  target of the restorable hook at `$05D8F5` (`LDA #$07 : STA $D0`), which takes the bank
  from here. `$0EF30C`-`$0EF30F` stay `$FF` for PIXI.
- `$0EF310` (flags, `bbBBVFCT`) is rewritten by every save for every level: `$08` (`V`)
  for a vanilla background, `$18` for one whose tiles take high byte 1 (vanilla's choice
  for data at or past `$0CE8FE`), `$00` for layer 2 objects.
  `$0EF600` (custom palettes) is not written; `$FF` fill means none.

### Game loop, stripe images, and the rest

- `$008072`: `JSR RunGameMode` in the game loop becomes `JMP $BA56`; `$00BA56`-`$00BA5C`
  (vanilla fill) calls `RunGameMode` and continues. With `LoadScrnImage`'s first
  instructions (`$0085D2`-`$0085DE`) rewritten, this is 3.70's faster stripe image upload
  (documented, changes 3.70; not in any corpus ROM).
- `$00FFD7`: the ROM size byte, `$0A` for 1 MiB; it follows the expansion, and PIXI, GPS,
  and UberASM Tool read it (`$0D` means an SA-1 ROM over 4 MiB).
- `$0FF035`-`$0FF083`: `$D8` then zeros after a fresh install (`$0FF035` is rewritten by
  every save, above). AddmusicK pads `$0F8000`-`$0FF050` with `$55`.
- `$0FFFE6`: set to `$01`. `$0FFFE7`-`$0FFFFF` are Lunar Magic's settings, written by every
  save (`$0FFFEB`, compression, is documented in lunar-magic.md). Unknown.

## RAM after a level load

What a level load leaves in a Lunar Magic-saved vanilla ROM and not in vanilla, over all
512 levels (`ramdiff.py --summary`). The last column says which piece leaves it, where an
ablation showed it. Kobo's one-time code must leave the same wherever Lunar Magic's
restorable code, or anything else, reads it; which reads exist is the main open question.

| RAM | Levels | Value | From |
|---|---|---|---|
| `$0BF6`-`$0C55` | all | 32 3-byte pointers to screen starts in `$7EC800`: `$7EC800 + n*rows*16` | `LoadBlkPtrs` entries point here |
| `$0C56`-`$0CB5` | all | the same into `$7FC800` | |
| `$0CB6`-`$0CD5`, `$0CD6`-`$0CF5` | horizontal | low and high bytes of the same offsets (vertical levels: 0) | replaces `DATA_00BA60`/`BA9C` |
| `$0CF6`-`$0D35` | all | per screen, a 16-bit address in the sprite list (bank `$CE`), 3 bytes on per sprite | |
| from `$0D37` | all | per screen, 16-bit, the number of sprites on the screens before it | |
| `$0BE7`, `$0BEE`-`$0BF5` | all | `$40` in most levels, `$00` in some (`1A`); `FF FF 30 FF C0 01 00 00` | |
| `$13D7`-`$13D8` | all | level height in pixels (`$01B0`, `$0100` vertical) | lunar-magic.md |
| `$1936`-`$1937` | all | height minus `$10` | |
| `$13CD` | all | see midway points | |
| `$010B`-`$010C`, `$00FE`-`$00FF` | all | level number; level number + 1 | hook `$05D8E2` |
| `$7FC00B` | all | the level's `$0EF310` flags | hook `$05803B` |
| `$7EB900`/`$7EBD00` | backgrounds | tilemap low and high bytes | hook `$05803B` |
| `$7FBC00`-`$7FBF5F`, `$7FC300`-`$7FC65F` | backgrounds | per background cell (both halves, 16x27), the 16-bit address of its Map16 definition in the BG Map16 bank | hook `$05803B` |
| `$05`-`$06`, `$0A`-`$0C` | backgrounds | stride and BG Map16 table | hook `$058DA4` |
| `$1BE6`-`$1DE7`, `$0695`-`$06B6`, `$7F819F` | most | upload buffers and state | the `$0580BF` retargets |
| `$7F8183`-`$7F819F` | all | `$FF` x16 then small values (vanilla: unused) | |

`$0BF6`-`$0D75` is vanilla's `GfxDecompSP1`, the buffer for sprite tiles `4A`-`4F` and
`5A`-`5F`; the VRAM patch moves those tiles (changes 3.60 mentions them). `$6B`-`$70`
(`Map16LowPtr`/`HighPtr`) also differ, as scratch.

## Unknowns, and how to find them

- Whether anything outside the one-time code reads the RAM above: Lunar Magic's
  restorable hooks (the VRAM patch at `$00F6E4` above all) or community patches.
  Method: build Kobo's one-time set, let Lunar Magic save the build (it adds its
  restorable hooks), and compare RAM after every level load, and pictures, with a Lunar
  Magic-saved vanilla ROM (`ramdiff.py`, `render_hashes`); a mismatch names the address.
- Behaviour that a level load does not exercise: tile changes in play (`$00C17A`,
  `$00C25C`, `GenerateTile`), block contact through the acts-like chain, scrolling
  (`$058A65` and siblings), screen shake, the overworld (`$04DCFA`, `$04E5F1`), the bonus
  and Yoshi wings exits, Choc Island 2's rooms, midway points, goal tapes. Method: a
  harness that sets up RAM, calls the vanilla routine that contains the hook in both ROMs
  (`expand::machine`), and compares RAM and VRAM afterwards; for the acts-like chain,
  with `$06F624` pointing at a table Kobo writes.
- `$13CD`, `$0BE7`, `$0BEE`-`$0BF5`, `$7F8183`-`$7F819F`, `$0FF035`, `$0FFFE6`: values
  observed, meaning not. Method: vary one level property at a time (height, layer 2,
  midway settings) with Lunar Magic's command line on a copy and diff RAM after load.
- How Lunar Magic's acts-like code tells an empty custom block slot, and what block tools
  other than GPS check there. Method: tool sources (Block Tool Super Deluxe, if any is
  published), and saving a ROM with a `JSL` written into one slot.
- Older versions: what 1.6x-2.x put at each piece (the corpus comparison only says equal
  to 3.70, vanilla, or other), and what makes 3.70 upgrade an older install. Kobo reads
  many versions but writes 3.70's layout, so this matters only for import.
- SA-1: the set on an SA-1 Pack ROM (SA-1 Pack remaps RAM inside several of these ranges,
  such as `$00C17D`, `$058A67`, `$01AC44`, `$00A2B0`). Method: the same spike on a vanilla
  ROM with SA-1 Pack applied.
- GUI operations (overworld save, ExAnimation, custom palettes, VRAM patch options) were
  not tried; they may write inside this set.
