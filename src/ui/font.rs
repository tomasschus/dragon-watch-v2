use embedded_graphics::{pixelcolor::Rgb888, prelude::*};

#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub code: u32,
    pub adv_w: u16,   // advance width in 1/16 px (LVGL convention)
    pub box_w: u16,
    pub box_h: u16,
    pub ofs_x: i16,
    pub ofs_y: i16,   // offset from baseline to glyph bottom (positive = above baseline)
    pub bmp_index: u32,
}

pub struct Font<'a> {
    pub glyphs: &'a [Glyph],
    pub bitmap: &'a [u8],
    pub line_height: u16,
    pub base_line: u16, // distance from bottom of line to baseline (LVGL convention)
}

pub fn calculate_text_width(font: &Font, text: &str) -> u32 {
    text.chars()
        .map(|c| {
            font.glyphs
                .iter()
                .find(|g| g.code == c as u32)
                .map_or(0, |g| (g.adv_w as u32 + 8) / 16)
        })
        .sum()
}

/// Draw antialiased text blending against the RGB565 background framebuffer.
/// `baseline`: pen position where x=left edge of first char, y=baseline y coordinate.
/// `bg`: raw RGB565 LE bytes of the background image (410*502*2 bytes).
pub fn draw_text<T>(
    target: &mut T,
    bg: &[u8],
    font: &Font,
    text: &str,
    baseline: Point,
    color: Rgb888,
) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb888>,
{
    draw_text_split(target, bg, font, text, baseline, color, color, 50)
}

/// Draw antialiased text blended against a solid `bg_color` (no framebuffer
/// sampling). Used for full-screen solid-background views like the study
/// summary mode, where there's no background image to read from.
pub fn draw_text_on_color<T>(
    target: &mut T,
    font: &Font,
    text: &str,
    baseline: Point,
    color: Rgb888,
    bg_color: Rgb888,
) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb888>,
{
    const SCREEN_W: i32 = 410;
    const SCREEN_H: i32 = 502;

    let mut pen_x = baseline.x;
    let baseline_y = baseline.y;

    for ch in text.chars() {
        let glyph = match font.glyphs.iter().find(|g| g.code == ch as u32) {
            Some(g) => g,
            None => {
                pen_x += (font.glyphs.first().map_or(0, |g| g.adv_w as i32) + 8) / 16;
                continue;
            }
        };

        let advance = (glyph.adv_w as i32 + 8) / 16;

        if glyph.box_w > 0 && glyph.box_h > 0 {
            let glyph_left = pen_x + glyph.ofs_x as i32;
            let glyph_bottom = baseline_y - glyph.ofs_y as i32;
            let glyph_top = glyph_bottom - glyph.box_h as i32;

            for row in 0..glyph.box_h as i32 {
                for col in 0..glyph.box_w as i32 {
                    let sx = glyph_left + col;
                    let sy = glyph_top + row;

                    if sx < 0 || sx >= SCREEN_W || sy < 0 || sy >= SCREEN_H {
                        continue;
                    }

                    let pixel_idx = (row * glyph.box_w as i32 + col) as usize;
                    let byte_idx = glyph.bmp_index as usize + pixel_idx / 4;
                    let bit_shift = 6u8.saturating_sub((pixel_idx % 4) as u8 * 2);
                    let coverage = if byte_idx < font.bitmap.len() {
                        (font.bitmap[byte_idx] >> bit_shift) & 0x3
                    } else {
                        0
                    };

                    if coverage == 0 {
                        continue;
                    }

                    let alpha: u32 = coverage as u32 * 85; // 0,85,170,255
                    let inv_a = 255 - alpha;
                    let out_r = (bg_color.r() as u32 * inv_a + color.r() as u32 * alpha) / 255;
                    let out_g = (bg_color.g() as u32 * inv_a + color.g() as u32 * alpha) / 255;
                    let out_b = (bg_color.b() as u32 * inv_a + color.b() as u32 * alpha) / 255;

                    Pixel(
                        Point::new(sx, sy),
                        Rgb888::new(out_r as u8, out_g as u8, out_b as u8),
                    )
                    .draw(target)?;
                }
            }
        }

        pen_x += advance;
    }

    Ok(())
}

/// `calculate_text_width`, scaled down by `scale_pct` (e.g. 80 = 80%).
/// Use the same `scale_pct` here and in `draw_text_on_color_scaled` so
/// wrapping and rendering agree on how wide each line actually is.
pub fn calculate_text_width_scaled(font: &Font, text: &str, scale_pct: u32) -> u32 {
    (calculate_text_width(font, text) * scale_pct) / 100
}

/// Like `draw_text_on_color`, but renders each glyph shrunk to `scale_pct`%
/// of its native size (nearest-neighbor downsample of the 2bpp coverage
/// bitmap). There's no smaller font asset available, so this is how the
/// study-summary view gets a smaller point size out of the existing fonts.
pub fn draw_text_on_color_scaled<T>(
    target: &mut T,
    font: &Font,
    text: &str,
    baseline: Point,
    color: Rgb888,
    bg_color: Rgb888,
    scale_pct: u32,
) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb888>,
{
    const SCREEN_W: i32 = 410;
    const SCREEN_H: i32 = 502;

    let mut pen_x = baseline.x;
    let baseline_y = baseline.y;
    let scale = scale_pct as i32;

    for ch in text.chars() {
        let glyph = match font.glyphs.iter().find(|g| g.code == ch as u32) {
            Some(g) => g,
            None => {
                let adv = (font.glyphs.first().map_or(0, |g| g.adv_w as i32) + 8) / 16;
                pen_x += adv * scale / 100;
                continue;
            }
        };

        let advance = ((glyph.adv_w as i32 + 8) / 16) * scale / 100;

        if glyph.box_w > 0 && glyph.box_h > 0 {
            let scaled_w = (glyph.box_w as i32 * scale / 100).max(1);
            let scaled_h = (glyph.box_h as i32 * scale / 100).max(1);
            let ofs_x_s = glyph.ofs_x as i32 * scale / 100;
            let ofs_y_s = glyph.ofs_y as i32 * scale / 100;

            let glyph_left = pen_x + ofs_x_s;
            let glyph_bottom = baseline_y - ofs_y_s;
            let glyph_top = glyph_bottom - scaled_h;

            for row in 0..scaled_h {
                let src_row = (row * glyph.box_h as i32) / scaled_h;
                for col in 0..scaled_w {
                    let src_col = (col * glyph.box_w as i32) / scaled_w;
                    let sx = glyph_left + col;
                    let sy = glyph_top + row;

                    if sx < 0 || sx >= SCREEN_W || sy < 0 || sy >= SCREEN_H {
                        continue;
                    }

                    let pixel_idx = (src_row * glyph.box_w as i32 + src_col) as usize;
                    let byte_idx = glyph.bmp_index as usize + pixel_idx / 4;
                    let bit_shift = 6u8.saturating_sub((pixel_idx % 4) as u8 * 2);
                    let coverage = if byte_idx < font.bitmap.len() {
                        (font.bitmap[byte_idx] >> bit_shift) & 0x3
                    } else {
                        0
                    };

                    if coverage == 0 {
                        continue;
                    }

                    let alpha: u32 = coverage as u32 * 85;
                    let inv_a = 255 - alpha;
                    let out_r = (bg_color.r() as u32 * inv_a + color.r() as u32 * alpha) / 255;
                    let out_g = (bg_color.g() as u32 * inv_a + color.g() as u32 * alpha) / 255;
                    let out_b = (bg_color.b() as u32 * inv_a + color.b() as u32 * alpha) / 255;

                    Pixel(
                        Point::new(sx, sy),
                        Rgb888::new(out_r as u8, out_g as u8, out_b as u8),
                    )
                    .draw(target)?;
                }
            }
        }

        pen_x += advance;
    }

    Ok(())
}

/// Like `draw_text` but `color_top` covers the top `split_pct`% of each glyph
/// and `color_bot` covers the remainder.
pub fn draw_text_split<T>(
    target: &mut T,
    bg: &[u8],
    font: &Font,
    text: &str,
    baseline: Point,
    color_top: Rgb888,
    color_bot: Rgb888,
    split_pct: u8,
) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb888>,
{
    const SCREEN_W: i32 = 410;
    const SCREEN_H: i32 = 502;

    let mut pen_x = baseline.x;
    let baseline_y = baseline.y;

    for ch in text.chars() {
        let glyph = match font.glyphs.iter().find(|g| g.code == ch as u32) {
            Some(g) => g,
            None => {
                pen_x += (font.glyphs.first().map_or(0, |g| g.adv_w as i32) + 8) / 16;
                continue;
            }
        };

        let advance = (glyph.adv_w as i32 + 8) / 16;

        if glyph.box_w > 0 && glyph.box_h > 0 {
            // glyph top-left in screen coords:
            // bottom of glyph box = baseline_y - ofs_y (positive ofs_y = above baseline)
            // top of glyph box = bottom - box_h
            let glyph_left = pen_x + glyph.ofs_x as i32;
            let glyph_bottom = baseline_y - glyph.ofs_y as i32;
            let glyph_top = glyph_bottom - glyph.box_h as i32;
            let split_row = (glyph.box_h as i32 * split_pct as i32) / 100;

            for row in 0..glyph.box_h as i32 {
                let color = if row < split_row { color_top } else { color_bot };

                for col in 0..glyph.box_w as i32 {
                    let sx = glyph_left + col;
                    let sy = glyph_top + row;

                    if sx < 0 || sx >= SCREEN_W || sy < 0 || sy >= SCREEN_H {
                        continue;
                    }

                    // Decode 2bpp coverage (MSB-first, 4 pixels per byte)
                    let pixel_idx = (row * glyph.box_w as i32 + col) as usize;
                    let byte_idx = glyph.bmp_index as usize + pixel_idx / 4;
                    let bit_shift = 6u8.saturating_sub((pixel_idx % 4) as u8 * 2);
                    let coverage = if byte_idx < font.bitmap.len() {
                        (font.bitmap[byte_idx] >> bit_shift) & 0x3
                    } else {
                        0
                    };

                    if coverage == 0 {
                        continue;
                    }

                    let alpha: u32 = coverage as u32 * 85; // 0,85,170,255

                    // Sample background pixel (RGB565 LE)
                    let bg_idx = (sy as usize * SCREEN_W as usize + sx as usize) * 2;
                    let (bg_r, bg_g, bg_b) = if bg_idx + 1 < bg.len() {
                        let px = u16::from_le_bytes([bg[bg_idx], bg[bg_idx + 1]]);
                        let r5 = (px >> 11) & 0x1F;
                        let g6 = (px >> 5) & 0x3F;
                        let b5 = px & 0x1F;
                        (
                            ((r5 << 3) | (r5 >> 2)) as u32,
                            ((g6 << 2) | (g6 >> 4)) as u32,
                            ((b5 << 3) | (b5 >> 2)) as u32,
                        )
                    } else {
                        (0, 0, 0)
                    };

                    // Alpha blend: out = bg*(255-alpha) + color*alpha / 255
                    let inv_a = 255 - alpha;
                    let out_r = (bg_r * inv_a + color.r() as u32 * alpha) / 255;
                    let out_g = (bg_g * inv_a + color.g() as u32 * alpha) / 255;
                    let out_b = (bg_b * inv_a + color.b() as u32 * alpha) / 255;

                    Pixel(
                        Point::new(sx, sy),
                        Rgb888::new(out_r as u8, out_g as u8, out_b as u8),
                    )
                    .draw(target)?;
                }
            }
        }

        pen_x += advance;
    }

    Ok(())
}
