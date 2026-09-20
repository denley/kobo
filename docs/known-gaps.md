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
  ROM code; an undrawn-sprite census over every level of the corpus runs without errors
  apart from levels whose loader already fails, but nothing compares custom sprites to an
  emulator. Boss arenas show the OAM of the first drawing pass instead.
- Sprites whose tiles are uploaded per frame into the player's dynamic tile area (the
  Podoboo; VRAM bytes `$C0C0`-`$C305` were seen to matter) are drawn from whatever the
  player pass left there: the sprite passes never run the NMI upload (`MarioGFXDMA`), and the
  scene is drawn from the level's VRAM, not the pass's.
- A sprite is captured alone, in the slot the loader gives it then, and some sprites look
  at their slot: the Yoshi's House birds take their colour from it, several animations
  their phase, and the line-guided rope its length (nine segments from slot 6 up under a
  sprite memory setting other than zero, else five). In the game the sprites of a screen
  share the slots, so such a sprite may come out in another colour, phase, or length than a
  player sees.
- SA-1 ROMs ([sa1.md](sa1.md)): character conversion DMA (SA-1 Pack's dynamic sprites), the
  SA-1's timers, and Super MMC bank switching while the game runs are not modelled. Of the
  39 SA-1 hacks in the corpus, 38 render all 512 levels; QLDC 2021 `76_Bench-kun` fails in
  the 18 Mode 7 boss rooms with a `COP` at `$00E296`, which is the hack's own doing (the
  patch at `$10E288` calls `$00987D` with `JSL` and the routine returns with `RTS`, into
  data). Nothing compares the hacks' pictures to an emulator.
- `GFX27`'s layout is unknown; `GFX32`/`GFX33` are not handled by the GFX tooling.
