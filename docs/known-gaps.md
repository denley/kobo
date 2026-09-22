# Known gaps

What a rendered level does not reproduce, and what the tooling does not handle.

- Layers 2 and 3 are drawn as the entry screen shows them and continued unstretched
  (see the layer facts in [smw.md](smw.md)); parallax is not reproduced away from the entry
  screen, the status bar is left out, and an axis layer 3 does not scroll along cannot be
  followed once the camera moves.
- Windows are applied only on a boss arena's fixed screen: they are screen positions, which
  a picture of a whole scrolling level has no place for. The spotlight rooms (mode `$11`)
  render uniformly dark, which is what the game shows until the light switch is hit (the
  spotlight sprite writes an empty window to `$04A0` while its `$C2` is zero), and the
  keyhole and message box effects do not appear.
- Sprites show their first drawn frame, each alone on a camera centred on it, with Mario
  off the left screen edge and no scrolling, so anything that waits for Mario or spawns
  over time (Bullet Bill shooters, generators, Lakitu's Spinies, a Magikoopa, Monty Moles
  in some hacks) is a marker or not what a player sees, and sprites that interact with one
  another only do so when they share a spot. A sprite that stays hidden at first is drawn
  where it first appears (a Podoboo at the lava's surface). Cluster sprites are captured
  from their spawner's camera only, except the candle flames. Custom sprite loaders run as
  ROM code; the full render sweep below records sprite-pass failures even in levels whose
  loader succeeds, superseding the earlier census's claim of no such errors. The emulator
  oracle compares which sprite is in which slot on a sample of every hack's levels
  ([testing.md](testing.md)), and whole frames on a few; nothing compares each custom
  sprite's own picture. Boss arenas show the OAM of the first drawing pass instead.
- A sprite is captured alone, and some sprites look at their slot: the Yoshi's House birds
  take their colour from it, several animations their phase, and the line-guided rope its
  length. It gets the slot the ROM's loader gives it with the columns before it loaded in
  order from the entrance and every sprite of them still in place, which is what a player
  sees who walks there and leaves everything alone. One who kills or outruns sprites, or
  arrives through another entrance, may see another colour, phase, or length.
- SA-1 ROMs ([sa1.md](sa1.md)): the SA-1's timers, the second type of character conversion,
  the variable-length bit reader, and write protection are not modelled; no hack in the
  corpus uses the first three, and the last only matters to a game that relies on a write
  being refused. The full render sweep below supersedes the earlier claim that 38 of 39
  SA-1 hacks render all 512 levels: `70_DPBOX` also has fatal errors, and successful PNGs
  can carry sprite-pass warnings. The emulator oracle compares tile grids, layer 3
  tilemaps, and sprite slots on a sample of each hack's levels ([testing.md](testing.md)),
  not their pictures.
- Code a hack runs every frame of the level loop (a custom status bar, a power-up handed
  to the player, UberASM `main` code) has not run: a level is loaded and prepared, and its
  sprites and player are drawn, but no frame of game mode `$14` is played.
- HDMA is not run, so whatever a hack changes by scanline is missing: a gradient sky
  (Luminescent level `148` writes the fixed colour per line) comes out as the one colour the
  level's back area has.
- The GFX tooling refuses ROMs their authors locked (`GfxError::Locked`, see
  [lunar-magic.md](lunar-magic.md)): their pointer tables are not addresses. Levels are
  unaffected, since the ROM's own decompression runs for them. There is no LC_LZ2 or
  LC_LZ3 encoder yet, which the build will need.

## Full hack render sweep: 2026-09-22

At revision `25cca50e847ee679c500e2a287ef3e71faf3322f`, the release CLI attempted all
512 slots (`000`–`1FF`) in each of 173 distinct hack ROMs, with sprites and the player
enabled. Of 88,576 attempts, 88,522 produced PNGs and 54 failed; 209 of the produced
PNGs carried diagnostics. Ten hacks had failures or warnings; 163 had neither.
Counts of warnings here mean affected level slots, not individual failed passes.
See [testing.md](testing.md#full-hack-render-sweep) for the inputs, logs and reproduction.

These are execution results, not a visual accuracy assessment. All slots were attempted,
including unused, test and unchanged vanilla rooms. Apart from the previously diagnosed
`76_Bench-kun` defect below, the causes have not been established: a failure is not yet
proof of a Kobo bug or of a broken playable level. A PNG with warnings may omit or
misrender the player or sprites; a PNG without warnings can still exhibit the visual
gaps listed above.

| Hack | Failed slots | PNGs with warnings | Observed problem |
| --- | ---: | ---: | --- |
| QLDC 2021 `70_DPBOX` | 24 | 196 | All fatal errors and most warning messages stop at `BRK at $11:0001`; sprite-loader passes in `101` and `13C` also stop at `$05:49BD`. |
| QLDC 2021 `34_idol` | 9 | 4 | Five slots stop at `BRK`; four exceed the 200-million-instruction limit. Sprite passes also fail in `1AC`–`1AF`. |
| QLDC 2021 `79_Hwailaluta` | 0 | 3 | Sprite or slotless-sprite passes in `105`, `108`, `109` stop at `BRK at $14:C922`. |
| Invictus 1.0 | 0 | 1 | `136`: player entrance and 16 more passes stop at `BRK at $93:9D9F`. |
| Luminescent v1.02 | 0 | 1 | `14A`: sprite loader with camera at `(4080, 3984)` stops at `BRK at $85:0011`. |
| QLDC 2021 `44_Daizo Dee Von` | 0 | 3 | `026`, `027`, `0C5`: sprite `00` at tile `(0, 0)` waits at `$9A:C13A` for something that never happens. |
| QLDC 2021 `77_NerDose` | 0 | 1 | `004`: sprite `73` at tile `(500, 31)` stops at `BRK at $70:0000`. |
| Grand Poo World 2 | 1 | 0 | `09F`: Lunar Magic background Map16 table pointer is null. |
| QLDC 2022 `04_Hwailaluta` | 2 | 0 | `09F`, `104`: Lunar Magic background Map16 table pointer is null. |
| QLDC 2021 `76_Bench-kun` | 18 | 0 | Known hack defect in Mode 7 boss rooms: `COP at $00:E296`, explained below. |

Investigate `70_DPBOX` first: the repeated address across loading and sprite passes
suggests a shared cause, but that cause has not been traced. Its 24 failed slots are
`095`–`09B`, `0CC`, `0D5`, `0D9`, `0DF`, `0E2`, `0E5`, `195`–`19B`, `1C7`, `1DE`,
`1EB`, `1F6`; `105` is one example that produces a PNG with warnings.

For `34_idol`, the instruction-limit failures are `016`, `08F`, `090`, `113`, at
`$90:8DAB`–`$90:8DB3`. The fatal `BRK` slots are `012`, `038`, `0B1`, `0EB`, `13F`,
at different addresses. These and the `79_Hwailaluta` sprite failures are the next
investigation targets. Check the null-background-pointer slots against their intended
use and emulator behaviour before deciding whether to change the parser.

The `76_Bench-kun` failure is already attributed to the hack: its patch at `$10E288`
calls `$00987D` with `JSL`, but the routine returns with `RTS`, into data. The sweep
reproduced it in `095`, `098`–`09B`, `0CC`, `0D5`, `0D9`, `0DF`, `0E2`, `0E5`,
`195`, `198`–`19B`, `1C7`, `1DE`. Do not treat those as evidence of a new CPU-core bug.
