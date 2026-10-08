use chrono::NaiveDate;
use fontdue::{
    layout::{CoordinateSystem, HorizontalAlign, Layout, LayoutSettings, TextStyle},
    Font, FontSettings,
};

pub const LABEL_WIDTH: u32 = 512;
pub const LABEL_HEIGHT: u32 = 96;

const FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/liberation/LiberationMono-Italic.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Oblique.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];
const INK: [u8; 3] = [58, 54, 50];

pub struct DateStamp {
    font: Font,
}

pub struct LabelImage {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl DateStamp {
    pub fn new() -> Result<Self, String> {
        for path in FONT_PATHS {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                    return Ok(Self { font });
                }
            }
        }
        Err("could not load a Polaroid date font; install Liberation Mono or DejaVu Sans".into())
    }

    pub fn draw(&self, date: NaiveDate) -> LabelImage {
        let mut pixels = vec![0; (LABEL_WIDTH * LABEL_HEIGHT * 4) as usize];
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 11.0,
            max_width: Some(LABEL_WIDTH as f32),
            horizontal_align: HorizontalAlign::Center,
            ..LayoutSettings::default()
        });
        let text = date.format("%b %-d, %Y").to_string();
        layout.append(&[&self.font], &TextStyle::new(&text, 60.0, 0));
        for glyph in layout.glyphs() {
            let (_, coverage) = self.font.rasterize_config(glyph.key);
            for gy in 0..glyph.height {
                for gx in 0..glyph.width {
                    let x = glyph.x as i32 + gx as i32;
                    let y = glyph.y as i32 + gy as i32;
                    if x < 0 || y < 0 || x >= LABEL_WIDTH as i32 || y >= LABEL_HEIGHT as i32 {
                        continue;
                    }
                    let offset = ((y as u32 * LABEL_WIDTH + x as u32) * 4) as usize;
                    pixels[offset..offset + 3].copy_from_slice(&INK);
                    pixels[offset + 3] = coverage[gy * glyph.width + gx];
                }
            }
        }
        LabelImage {
            pixels,
            width: LABEL_WIDTH,
            height: LABEL_HEIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DateStamp;

    #[test]
    fn date_stamp_draws_ink() {
        let stamp = DateStamp::new().unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let image = stamp.draw(date);
        assert!(image.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }
}
