use std::borrow::Cow;
use std::ptr;

use crossfont::{BitmapBuffer, RasterizedGlyph};

use crate::gl;
use crate::gl::types::*;

use super::Glyph;

/// Size of the Atlas.
pub const ATLAS_SIZE: i32 = 1024;

/// Manages a single texture atlas.
///
/// The strategy for filling an atlas looks roughly like this:
///
/// ```text
///                           (width, height)
///   ┌─────┬─────┬─────┬─────┬─────┐
///   │ 10  │     │     │     │     │ <- Empty spaces; can be filled while
///   │     │     │     │     │     │    glyph_height < height - row_baseline
///   ├─────┼─────┼─────┼─────┼─────┤
///   │ 5   │ 6   │ 7   │ 8   │ 9   │
///   │     │     │     │     │     │
///   ├─────┼─────┼─────┼─────┴─────┤ <- Row height is tallest glyph in row; this is
///   │ 1   │ 2   │ 3   │ 4         │    used as the baseline for the following row.
///   │     │     │     │           │ <- Row considered full when next glyph doesn't
///   └─────┴─────┴─────┴───────────┘    fit in the row.
/// (0, 0)  x->
/// ```
#[derive(Debug)]
pub struct Atlas {
    /// Texture id for this atlas.
    id: GLuint,

    /// Width of atlas.
    width: i32,

    /// Height of atlas.
    height: i32,

    /// Left-most free pixel in a row.
    ///
    /// This is called the extent because it is the upper bound of used pixels
    /// in a row.
    row_extent: i32,

    /// Baseline for glyphs in the current row.
    row_baseline: i32,

    /// Tallest glyph in current row.
    ///
    /// This is used as the advance when end of row is reached.
    row_tallest: i32,

    /// Gles context.
    ///
    /// This affects the texture loading.
    is_gles_context: bool,
}

/// Error that can happen when inserting a texture to the Atlas.
pub enum AtlasInsertError {
    /// Texture atlas is full.
    Full,

    /// The glyph cannot fit within a single texture.
    GlyphTooLarge,
}

impl Atlas {
    pub fn new(size: i32, is_gles_context: bool) -> Self {
        let mut id: GLuint = 0;
        unsafe {
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::GenTextures(1, &raw mut id);
            gl::BindTexture(gl::TEXTURE_2D, id);
            // Use RGBA texture for both normal and emoji glyphs, since it has no performance
            // impact.
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                size,
                size,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                ptr::null(),
            );

            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);

            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        Self {
            id,
            width: size,
            height: size,
            row_extent: 0,
            row_baseline: 0,
            row_tallest: 0,
            is_gles_context,
        }
    }

    pub fn clear(&mut self) {
        self.row_extent = 0;
        self.row_baseline = 0;
        self.row_tallest = 0;
    }

    /// Insert a `RasterizedGlyph` into the texture atlas.
    pub fn insert(&mut self, glyph: &RasterizedGlyph) -> Result<Glyph, AtlasInsertError> {
        if glyph.width > self.width || glyph.height > self.height {
            return Err(AtlasInsertError::GlyphTooLarge);
        }

        // If there's not enough room in current row, go onto next one.
        if !self.room_in_row(glyph) {
            self.advance_row()?;
        }

        // If there's still not room, there's nothing that can be done here..
        if !self.room_in_row(glyph) {
            return Err(AtlasInsertError::Full);
        }

        // There appears to be room; load the glyph.
        Ok(self.insert_inner(glyph))
    }

    /// Insert the glyph without checking for room.
    ///
    /// Internal function for use once atlas has been checked for space. GL
    /// errors could still occur at this point if we were checking for them;
    /// hence, the Result.
    fn insert_inner(&mut self, glyph: &RasterizedGlyph) -> Glyph {
        let offset_y = self.row_baseline;
        let offset_x = self.row_extent;
        let height = glyph.height;
        let width = glyph.width;
        let multicolor;

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.id);

            // Load data into OpenGL.
            let (format, buffer) = match &glyph.buffer {
                BitmapBuffer::Rgb(buffer) => {
                    multicolor = false;
                    // Gles context doesn't allow uploading RGB data into RGBA texture, so need
                    // explicit copy.
                    if self.is_gles_context {
                        let mut new_buffer = Vec::with_capacity(buffer.len() / 3 * 4);
                        for rgb in buffer.chunks_exact(3) {
                            new_buffer.push(rgb[0]);
                            new_buffer.push(rgb[1]);
                            new_buffer.push(rgb[2]);
                            new_buffer.push(u8::MAX);
                        }
                        (gl::RGBA, Cow::Owned(new_buffer))
                    } else {
                        (gl::RGB, Cow::Borrowed(buffer))
                    }
                },
                BitmapBuffer::Rgba(buffer) => {
                    multicolor = true;
                    (gl::RGBA, Cow::Borrowed(buffer))
                },
            };

            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                offset_x,
                offset_y,
                width,
                height,
                format,
                gl::UNSIGNED_BYTE,
                buffer.as_ptr().cast(),
            );

            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        // Update Atlas state.
        self.row_extent = offset_x + width;
        if height > self.row_tallest {
            self.row_tallest = height;
        }

        // Generate UV coordinates.
        let uv_bot = offset_y as f32 / self.height as f32;
        let uv_left = offset_x as f32 / self.width as f32;
        let uv_height = height as f32 / self.height as f32;
        let uv_width = width as f32 / self.width as f32;

        Glyph {
            tex_id: self.id,
            multicolor,
            top: glyph.top as i16,
            left: glyph.left as i16,
            width: width as i16,
            height: height as i16,
            uv_bot,
            uv_left,
            uv_width,
            uv_height,
        }
    }

    /// Check if there's room in the current row for given glyph.
    pub fn room_in_row(&self, raw: &RasterizedGlyph) -> bool {
        let next_extent = self.row_extent + raw.width;
        let enough_width = next_extent <= self.width;
        let enough_height = raw.height <= self.height - self.row_baseline;

        enough_width && enough_height
    }

    /// Mark current row as finished and prepare to insert into the next row.
    pub fn advance_row(&mut self) -> Result<(), AtlasInsertError> {
        let advance_to = self.row_baseline + self.row_tallest;
        if self.height - advance_to <= 0 {
            return Err(AtlasInsertError::Full);
        }

        self.row_baseline = advance_to;
        self.row_extent = 0;
        self.row_tallest = 0;

        Ok(())
    }
}

/// Owns texture-atlas storage, insertion position, and GL texture-binding state.
#[derive(Debug)]
pub struct AtlasCache {
    atlases: Vec<Atlas>,
    current: usize,
    active_texture: GLuint,
}

impl AtlasCache {
    pub fn new(is_gles_context: bool) -> Self {
        Self {
            atlases: vec![Atlas::new(ATLAS_SIZE, is_gles_context)],
            current: 0,
            active_texture: 0,
        }
    }

    /// Load a glyph, allocating another atlas only when every existing atlas is full.
    #[inline]
    pub fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        loop {
            match self.atlases[self.current].insert(rasterized) {
                Ok(glyph) => {
                    // Atlas uploads leave texture zero bound.
                    self.active_texture = 0;
                    return glyph;
                },
                Err(AtlasInsertError::Full) => {
                    let is_gles_context = self.atlases[self.current].is_gles_context;

                    self.current += 1;
                    if self.current == self.atlases.len() {
                        self.atlases.push(Atlas::new(ATLAS_SIZE, is_gles_context));
                        self.active_texture = 0;
                    }
                },
                Err(AtlasInsertError::GlyphTooLarge) => {
                    return Glyph {
                        tex_id: self.atlases[self.current].id,
                        multicolor: false,
                        top: 0,
                        left: 0,
                        width: 0,
                        height: 0,
                        uv_bot: 0.,
                        uv_left: 0.,
                        uv_width: 0.,
                        uv_height: 0.,
                    };
                },
            }
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.atlases.truncate(1);
        if let Some(atlas) = self.atlases.first_mut() {
            atlas.clear();
        }
        self.current = 0;
        self.active_texture = 0;
    }

    pub fn bind_texture(&mut self, texture: GLuint) {
        if self.active_texture == texture {
            return;
        }

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, texture);
        }
        self.active_texture = texture;
    }
}

impl Drop for Atlas {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &raw const self.id);
        }
    }
}
