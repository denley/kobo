//! Rendering a level in one call: load it, capture its sprites, and draw
//! everything in the order the picture needs.

use thiserror::Error;

use super::level_layers;
use super::{LayerTiles, compose_level, draw_objects, draw_sprite_marker, draw_sprite_scene};
use crate::expand::{self, Diagnostic, ExpandError, LoadedLevel};
use crate::image::RgbImage;
use crate::rom::Rom;
use crate::sprites::{self, SpriteError};

/// How a level's sprites are shown.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Sprites {
    /// As the game's sprite engine draws them, with an ID marker for
    /// every entry that draws nothing.
    #[default]
    Drawn,
    /// Every entry as an ID marker, without running the sprite engine.
    Markers,
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RenderOptions {
    pub sprites: Sprites,
    /// Draw the player where the level is entered.
    pub player: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            sprites: Sprites::Drawn,
            player: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    Expand(#[from] ExpandError),
    #[error(transparent)]
    Sprites(#[from] SpriteError),
}

/// A rendered level, with what it was rendered from.
#[derive(Debug)]
pub struct LevelRender {
    pub image: RgbImage,
    pub level: LoadedLevel,
    /// Every pass the CPU core gave up on, from loading and from the
    /// sprite capture; see [`expand::summarize`].
    pub diagnostics: Vec<Diagnostic>,
}

/// Loads `level` by running the ROM's own code and renders it.
pub fn render_level(
    rom: &Rom,
    level: u16,
    options: RenderOptions,
) -> Result<LevelRender, RenderError> {
    let level = expand::expand_level(rom, level)?;
    let (image, scene_diagnostics) = render_loaded(rom, &level, options)?;
    let mut diagnostics = level.diagnostics.clone();
    diagnostics.extend(scene_diagnostics);
    Ok(LevelRender {
        image,
        level,
        diagnostics,
    })
}

/// Renders a level that is already loaded, returning the picture and the
/// sprite capture's diagnostics. Graphics and colours come from what the
/// game uploaded to VRAM and CGRAM, so ExGFX, custom palettes, and
/// animated tiles are covered. A boss arena is its own fixed screen,
/// whose drawing pass already holds its sprites and the player.
pub fn render_loaded(
    rom: &Rom,
    level: &LoadedLevel,
    options: RenderOptions,
) -> Result<(RgbImage, Vec<Diagnostic>), RenderError> {
    let video = &level.video;
    let mut layers = level_layers(level, &LayerTiles::from_vram(&video.vram));
    let mut markers: Vec<(usize, usize, u8)> = Vec::new();
    let mut diagnostics = Vec::new();
    if options.sprites != Sprites::Hidden && level.scene.boss.is_none() {
        let list = sprites::read_sprites_at(rom, level.sprite_data_ptr())?;
        if options.sprites == Sprites::Markers {
            markers.extend(list.sprites.iter().map(|sprite| {
                let (x, y) = sprite.tile_position(level.tiles.vertical);
                (x, y, sprite.id)
            }));
        } else {
            let scene = expand::capture_sprites(rom, level, &list)?;
            draw_sprite_scene(
                &mut layers,
                &scene,
                level.scene.layer2_offset(),
                &video.vram,
            );
            markers = scene.undrawn;
            diagnostics = scene.diagnostics;
        }
    }
    // The player's OAM slots follow most of the sprites', so he goes
    // behind them: objects already in the layer stay in front.
    if options.player {
        draw_objects(
            &mut layers,
            &level.scene.player,
            video.object_select,
            &video.vram,
        );
    }
    let mut image = compose_level(level, &layers, &video.palette());
    for (x, y, id) in markers {
        draw_sprite_marker(&mut image, x as u32 * 16, y as u32 * 16, id, &video.vram);
    }
    Ok((image, diagnostics))
}
