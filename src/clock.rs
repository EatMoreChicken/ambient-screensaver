use chrono::{DateTime, Local, TimeZone};
use fontdue::{
    layout::{CoordinateSystem, HorizontalAlign, Layout, LayoutSettings, TextStyle},
    Font, FontSettings,
};

const FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];
const TEXT_COLOR: [u8; 3] = [68, 68, 68];

pub struct Clock {
    font: Font,
}

pub struct ClockImage {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub rect: [f32; 4],
}

fn label<Tz: TimeZone>(now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    now.format("%A, %B %-d, %Y  ·  %-I:%M %p").to_string()
}

impl Clock {
    pub fn new() -> Result<Self, String> {
        for path in FONT_PATHS {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                    return Ok(Self { font });
                }
            }
        }
        Err("could not load a clock font; install DejaVu Sans or Liberation Sans".into())
    }

    pub fn draw(
        &self,
        now: &DateTime<Local>,
        screen: (u32, u32),
        bottom_mat_fraction: f32,
    ) -> ClockImage {
        let width = screen.0;
        let height = (screen.1 as f32 * 0.04).floor().max(1.0) as u32;
        let font_size = (screen.1 as f32 * 0.019)
            .min(screen.0 as f32 * 0.028)
            .min(height as f32 * 0.62);
        let bottom_mat_height = screen.1 as f32 * bottom_mat_fraction;
        let rect = [
            0.0,
            screen.1 as f32 - (bottom_mat_height + height as f32) / 2.0,
            width as f32,
            height as f32,
        ];
        let mut pixels = vec![0; (width * height * 4) as usize];

        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: ((height as f32 - font_size * 1.2) / 2.0).max(0.0),
            max_width: Some(width as f32),
            horizontal_align: HorizontalAlign::Center,
            ..LayoutSettings::default()
        });
        layout.append(&[&self.font], &TextStyle::new(&label(now), font_size, 0));
        for glyph in layout.glyphs() {
            let (_, coverage) = self.font.rasterize_config(glyph.key);
            for gy in 0..glyph.height {
                for gx in 0..glyph.width {
                    let x = glyph.x as i32 + gx as i32;
                    let y = glyph.y as i32 + gy as i32;
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let offset = ((y as u32 * width + x as u32) * 4) as usize;
                    pixels[offset..offset + 3].copy_from_slice(&TEXT_COLOR);
                    pixels[offset + 3] = coverage[gy * glyph.width + gx];
                }
            }
        }

        ClockImage {
            pixels,
            width,
            height,
            rect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{label, Clock};

    #[test]
    fn formats_one_human_readable_line() {
        let date = chrono::DateTime::parse_from_rfc3339("2026-10-07T15:04:00-04:00").unwrap();
        assert_eq!(label(&date), "Wednesday, October 7, 2026  ·  3:04 PM");
    }

    #[test]
    fn clock_text_is_centered_in_a_transparent_bottom_strip() {
        let clock = Clock::new().unwrap();
        let image = clock.draw(&chrono::Local::now(), (1280, 800), 0.07);
        let slide_image = clock.draw(&chrono::Local::now(), (1280, 800), 0.04);
        assert_eq!(image.rect, [0.0, 756.0, 1280.0, 32.0]);
        assert_eq!(slide_image.rect, [0.0, 768.0, 1280.0, 32.0]);
        assert_eq!(image.rect[1] + image.rect[3] / 2.0, 772.0);
        assert_eq!(image.pixels[3], 0);
        let ink_x: Vec<_> = (0..image.width)
            .filter(|&x| {
                (0..image.height).any(|y| {
                    let offset = ((y * image.width + x) * 4 + 3) as usize;
                    image.pixels[offset] > 0
                })
            })
            .collect();
        assert!(!ink_x.is_empty());
        let center = (ink_x[0] + ink_x[ink_x.len() - 1]) as i32 / 2;
        assert!((center - 640).abs() <= 2);
    }
}
