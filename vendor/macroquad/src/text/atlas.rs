use crate::{get_context, get_quad_context, math::Rect, texture::Image, Color};

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

thread_local! {
    /// minebound patch: every shared atlas (fonts, the ui), flushed before each frame is drawn
    static SHARED: std::cell::RefCell<Vec<Weak<Mutex<Atlas>>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Keeps `atlas` in the list `flush_shared` sends up, and gives it back.
pub(crate) fn share(atlas: Atlas) -> Arc<Mutex<Atlas>> {
    let atlas = Arc::new(Mutex::new(atlas));
    SHARED.with(|s| {
        let mut s = s.borrow_mut();
        s.retain(|w| w.strong_count() > 0);
        s.push(Arc::downgrade(&atlas));
    });
    atlas
}

/// Uploads every shared atlas that changed since the last frame.
pub(crate) fn flush_shared() {
    SHARED.with(|s| {
        for atlas in s.borrow().iter().filter_map(Weak::upgrade) {
            atlas.lock().unwrap().flush();
        }
    });
}

#[derive(Debug, Clone, Copy)]
pub struct Sprite {
    pub rect: Rect,
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum SpriteKey {
    Texture(miniquad::TextureId),
    Id(u64),
}
pub struct Atlas {
    texture: miniquad::TextureId,
    image: Image,
    pub sprites: HashMap<SpriteKey, Sprite>,
    /// minebound patch: glyphs sit on shelves (top, height, next free x), each holding glyphs of
    /// about one size. One row for every size, as before, left most of a row above small text empty.
    shelves: Vec<(u16, u16, u16)>,
    /// where the next shelf starts
    bottom: u16,
    /// the rows changed since the last upload, sent up alone instead of the whole atlas
    changed: Option<(u16, u16)>,

    pub dirty: bool,

    filter: miniquad::FilterMode,

    unique_id: u64,
}

impl Drop for Atlas {
    fn drop(&mut self) {
        let ctx = &mut get_context().quad_context;
        ctx.delete_texture(self.texture);
    }
}

impl Atlas {
    // pixel gap between glyphs in the atlas
    const GAP: u16 = 2;
    // well..
    const UNIQUENESS_OFFSET: u64 = 100000;

    pub fn new(ctx: &mut dyn miniquad::RenderingBackend, filter: miniquad::FilterMode) -> Atlas {
        let image = Image::gen_image_color(512, 512, Color::new(0.0, 0.0, 0.0, 0.0));
        let texture = ctx.new_texture_from_rgba8(image.width, image.height, &image.bytes);
        ctx.texture_set_filter(
            texture,
            miniquad::FilterMode::Nearest,
            miniquad::MipmapFilterMode::None,
        );

        Atlas {
            image,
            texture,
            shelves: vec![],
            bottom: 0,
            changed: None,
            dirty: false,
            sprites: HashMap::new(),
            filter,
            unique_id: Self::UNIQUENESS_OFFSET,
        }
    }

    pub fn new_unique_id(&mut self) -> SpriteKey {
        self.unique_id += 1;

        SpriteKey::Id(self.unique_id)
    }

    pub fn set_filter(&mut self, filter_mode: miniquad::FilterMode) {
        let ctx = get_quad_context();
        self.filter = filter_mode;
        ctx.texture_set_filter(self.texture, filter_mode, miniquad::MipmapFilterMode::None);
    }

    pub fn get(&self, key: SpriteKey) -> Option<Sprite> {
        self.sprites.get(&key).cloned()
    }

    pub const fn width(&self) -> u16 {
        self.image.width
    }

    pub const fn height(&self) -> u16 {
        self.image.height
    }

    /// minebound patch: no upload here. Drawing text called this after every new glyph and sent
    /// the whole atlas (up to 16 MB) each time, so a screen of new text meant hundreds of MB the
    /// GPU driver held on to. The atlas goes up once, in `flush`, just before the frame is drawn.
    pub fn texture(&mut self) -> miniquad::TextureId {
        self.texture
    }

    /// Sends the atlas to the GPU if it changed: the changed rows only, or all of it once it grew.
    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let ctx = get_quad_context();
        let (texture_width, texture_height) = ctx.texture_size(self.texture);
        let row = self.image.width as usize * 4;
        if texture_width != self.image.width as _ || texture_height != self.image.height as _ {
            ctx.texture_resize(self.texture, self.image.width as _, self.image.height as _, Some(&self.image.bytes[..]));
        } else if let Some((top, end)) = self.changed {
            let rows = &self.image.bytes[top as usize * row..end as usize * row];
            ctx.texture_update_part(self.texture, 0, top as _, self.image.width as _, (end - top) as _, rows);
        }
        self.changed = None;
    }

    pub fn get_uv_rect(&self, key: SpriteKey) -> Option<Rect> {
        let ctx = get_quad_context();
        self.get(key).map(|sprite| {
            let (w, h) = ctx.texture_size(self.texture);

            Rect::new(
                sprite.rect.x / w as f32,
                sprite.rect.y / h as f32,
                sprite.rect.w / w as f32,
                sprite.rect.h / h as f32,
            )
        })
    }

    /// Where a `w` x `h` slot goes: the snuggest shelf with room, or a new shelf at the bottom.
    fn place(&mut self, w: u16, h: u16) -> Option<(u16, u16)> {
        let width = self.image.width;
        // a shelf takes glyphs a little shorter than itself, not much: that space would be lost
        let fit = (0..self.shelves.len())
            .filter(|&i| {
                let (_, sh, sx) = self.shelves[i];
                sh >= h && sh <= h + h / 4 + 2 && sx as u32 + w as u32 <= width as u32
            })
            .min_by_key(|&i| self.shelves[i].1);
        let i = match fit {
            Some(i) => i,
            None if self.bottom as u32 + h as u32 <= self.image.height as u32 && w <= width => {
                self.shelves.push((self.bottom, h, 0));
                self.bottom += h;
                self.shelves.len() - 1
            }
            None => return None,
        };
        let (y, _, x) = self.shelves[i];
        self.shelves[i].2 += w;
        Some((x, y))
    }

    /// Doubles the atlas's shorter side (or its width, for a glyph wider than it). Everything
    /// already placed keeps its spot, so nothing is packed again.
    fn grow(&mut self, glyph_width: u16) {
        let (w, h) = (self.image.width, self.image.height);
        let (nw, nh) = if glyph_width > w || w < h { (w * 2, h) } else { (w, h * 2) };
        let mut bytes = vec![0u8; nw as usize * nh as usize * 4];
        for (y, old) in self.image.bytes.chunks_exact(w as usize * 4).enumerate() {
            let at = y * nw as usize * 4;
            bytes[at..at + old.len()].copy_from_slice(old);
        }
        self.image = Image { width: nw, height: nh, bytes };
        self.dirty = true;
    }

    pub fn cache_sprite(&mut self, key: SpriteKey, sprite: Image) {
        let (width, height) = (sprite.width, sprite.height);
        let (x, y) = loop {
            if let Some(at) = self.place(width + Self::GAP, height + Self::GAP) {
                break at;
            }
            self.grow(width + Self::GAP);
        };

        self.dirty = true;
        let (row, line) = (self.image.width as usize * 4, width as usize * 4);
        for j in 0..height as usize {
            let at = (y as usize + j) * row + x as usize * 4;
            self.image.bytes[at..at + line].copy_from_slice(&sprite.bytes[j * line..(j + 1) * line]);
        }
        let end = y + height;
        self.changed = Some(self.changed.map_or((y, end), |(top, bottom)| (top.min(y), bottom.max(end))));

        self.sprites.insert(
            key,
            Sprite {
                rect: Rect::new(x as f32, y as f32, width as f32, height as f32),
            },
        );
    }
}
