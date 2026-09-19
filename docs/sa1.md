# SA-1

What an SA-1 ROM changes for the renderer, and how it is modelled. An SA-1 cartridge running
SMW is SA-1 Pack (Vitor Vilela): the game does not run on the chip without it. Its source is
the reference (`~/.local/share/kobo/docs/sa1pack/`, a clone of `VitorVilela7/SA1-Pack`, not
committed); `docs/memory-map-summary.md`, `docs/Sprite-Remap.md`, and `docs/maxtile.md` in it
are the ones to read.

## Recognising one

- Map mode `$23` at `$00FFD5` gives `Mapping::Sa1Rom`, and `RamMap::of` takes that to mean
  `RamMap::Sa1Pack`. Nothing looks for SA-1 Pack itself.
- A reference ROM is vanilla with SA-1 Pack applied, which should load every level as vanilla
  does: copy the headerless vanilla ROM and run `asar sa1.asm rom.sfc` in the source's `asm/`
  (Asar 1.91 works). It lives in `~/.local/share/kobo/roms/sa1/` and, like any ROM, is never
  committed. SA-1 Pack's guide says to save a level in Lunar Magic afterwards; the reference
  ROM has not had that done.

## The two processors

- The SA-1 is a second 65816 with its own view of the bus: I-RAM at `$0000`-`$07FF` and
  `$3000`-`$37FF`, BW-RAM in banks `$40`-`$4F` and through a window at `$6000`-`$7FFF`
  (`$2224` for the S-CPU, `$2225` for the SA-1, whose bit 7 selects the bitmap view), BW-RAM
  as 2- or 4-bit cells in banks `$60`-`$6F`, the ROM, its registers, and nothing of the
  console: no work RAM, no PPU. It takes its reset and IRQ vectors from `$2203`/`$2207`, and
  can replace the S-CPU's IRQ vector (`$220E`, selected by `$2209` bit 6). SA-1 Pack does:
  its S-CPU IRQ handler is in work RAM at `$1D00`.
- SA-1 Pack's calls are IRQs with a mailbox. S-CPU to SA-1: pointer in `$3180`-`$3182`,
  `JSR $1E80` (work RAM) writes `#$80` to `$2200` and spins on `$3189`; the SA-1's handler
  does `JML [$3180]` and increments `$0189` (the same byte: I-RAM is at both). SA-1 to
  S-CPU, used where the SA-1 needs the console (`stripe_help`, `score_stuff` in
  `boost/level_mode.asm`): pointer in `$0183`, `#$D0` to `$2209`, spin on `$018A`. Code
  tells which processor it is on from the stack page (`$37` is the SA-1's).
- Only one processor runs at a time (`cpu::smw_bus`): the SA-1 gets a turn when the S-CPU
  stops to wait, and runs until it waits itself. `Cpu::run` recognises a wait as `WAI` or as
  two passes round a loop with the registers and the CPU's own write count unchanged. The
  turn must not come when the S-CPU *triggers* the SA-1: `snes_init` releases the SA-1 from
  reset and then clears `$3189`, relying on the SA-1's start-up taking longer than that,
  and a SA-1 run at the trigger has already answered by then.
- An S-CPU IRQ needs interrupts enabled, so `Machine` enters every routine with `I` clear,
  as game-mode code runs. The direct page is `$3000` (`RamMap::direct_page`).
- Boot goes through the reset vector, in emulation mode. SA-1 Pack points the vector at
  `snes_init`, which sets the SA-1 up; entered in native mode it takes itself for a second
  boot on a swapped ROM image and flips the Super MMC bank bits.
- A fault on the SA-1 (`BRK`, `STP`, the step limit) stops it, and the S-CPU's wait for it
  then fails; `Machine` reports the SA-1's error (`CpuError::Sa1`) instead of the wait.
  The SA-1's state is part of `Ram` so that restoring a snapshot after a failed pass brings
  back a SA-1 that is idle, not one stuck in the middle of a handler.

## What SA-1 Pack moves

- RAM (`ram::RamMap::Sa1Pack`): `$7E0000`-`$7E00FF` to I-RAM `$3000`, `$7E0100`-`$7E1FFF` to
  `$400100`, the tile grid `$7EC800`/`$7FC800` to `$40C800`/`$41C800`, Wiggler segments to
  `$418800`, the sprite load flags `$1938` to `$418A00` (255 of them). The rest of work RAM
  stays, for the S-CPU alone. The per-slot sprite tables are packed at `$3200` and `$74C8`
  with 22 slots each; `$9E`, `$D8`, and `$E4` left the direct page, and `$AA` and `$C2` moved
  into the space (`$9E` and `$D8`).
- Every vanilla level's sprite memory setting is rewritten to `$08` in the ROM
  (`remap/sprite_memory.asm`), bar the boss and Wiggler ones, so slots are given out
  differently from vanilla.
- Graphics are decompressed by the SA-1 into BW-RAM at `$410000`, then copied into work RAM
  by the S-CPU with a DMA to the work RAM port (`$2180`-`$2183`), which the bus therefore
  models. (LoROM hacks use the port too: Invictus's custom sprite `D1` draws nothing
  without it.)
- `ConsolidateOAM` (`$008494`, where every drawing pass ends) becomes MaxTile's
  `oam_compress`, run on the SA-1: it rebuilds all of `$0200`-`$03FF` from four priority
  buffers, packed down from `$03FC`. After a frame no object is where the game drew it, so
  whatever is found by its OAM index (the player in objects 64-71, the candle flames in
  124-127) is read when the frame gets to `$008494`, with sizes from the unpacked table at
  `$0420` (`expand::oam::draw_frame`). MaxTile also changes which sprite is in front of
  which, deliberately.
- The OAM upload no longer applies `$3F` (`org $00846A : RTS`); MaxTile puts the objects
  from `$3F` on in its first buffer instead.

## Not modelled

Timers, SA-1 DMA and character conversion (SA-1 Pack uses it in NMI for dynamic sprites),
the variable-length bit reader, write protection, the SA-1's NMI, and Super MMC bank
switching: ROM reads use the default assignment, so an image over 4 MiB (`8mb.asm` maps
banks `$C0`-`$FF` to the second half) reads the wrong data there.
