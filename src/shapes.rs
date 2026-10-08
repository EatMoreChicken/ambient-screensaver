use rand::Rng;

pub const SPRITE_SIZE: u32 = 192;
pub const FLOATING_SHAPE_COUNT: usize = 6;

pub const PASTELS: [[u8; 3]; 8] = [
    [246, 204, 220], // rose
    [255, 217, 185], // peach
    [248, 231, 173], // butter
    [196, 232, 210], // mint
    [194, 224, 247], // sky
    [222, 204, 247], // lavender
    [184, 231, 227], // seafoam
    [205, 211, 249], // periwinkle
];

#[derive(Clone, Copy, Debug)]
pub enum ShapeKind {
    Circle,
    Oval,
    Donut,
    Squiggle,
    Capsule,
    Blob,
}

impl ShapeKind {
    pub const ALL: [Self; 6] = [
        Self::Circle,
        Self::Oval,
        Self::Donut,
        Self::Squiggle,
        Self::Capsule,
        Self::Blob,
    ];
}

fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

fn shape_distance(kind: ShapeKind, x: f32, y: f32) -> f32 {
    let distance = x.hypot(y);
    match kind {
        ShapeKind::Circle => distance - 0.64,
        ShapeKind::Oval => ((x / 0.75).hypot(y / 0.47) - 1.0) * 0.47,
        ShapeKind::Donut => (distance - 0.57).abs() - 0.105,
        ShapeKind::Squiggle => {
            let mut nearest = f32::MAX;
            let point = [x, y];
            for index in 0..40 {
                let t0 = index as f32 / 40.0;
                let t1 = (index + 1) as f32 / 40.0;
                let curve = |t: f32| {
                    [
                        -0.72 + 1.44 * t,
                        0.23 * (t * std::f32::consts::TAU * 1.5).sin(),
                    ]
                };
                nearest = nearest.min(segment_distance(point, curve(t0), curve(t1)));
            }
            nearest - 0.085
        }
        ShapeKind::Capsule => segment_distance([x, y], [-0.53, 0.23], [0.53, -0.23]) - 0.19,
        ShapeKind::Blob => {
            let angle = y.atan2(x);
            distance - (0.58 + 0.07 * (3.0 * angle + 0.4).sin() + 0.035 * (5.0 * angle).cos())
        }
    }
}

pub fn rasterize(kind: ShapeKind, color: [u8; 3]) -> Vec<u8> {
    let size = SPRITE_SIZE as usize;
    let mut pixels = vec![0; size * size * 4];
    let edge = 2.0 / SPRITE_SIZE as f32;
    for row in 0..size {
        for col in 0..size {
            let x = (col as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let y = (row as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let coverage = ((edge - shape_distance(kind, x, y)) / (edge * 2.0)).clamp(0.0, 1.0);
            let coverage = coverage * coverage * (3.0 - 2.0 * coverage);
            let index = (row * size + col) * 4;
            pixels[index..index + 3].copy_from_slice(&color);
            pixels[index + 3] = (coverage * 255.0).round() as u8;
        }
    }
    pixels
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    BehindPhotos,
    OverPhotos,
}

pub struct Motion {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub vx: f32,
    pub vy: f32,
    pub angle: f32,
    pub spin: f32,
    pub opacity: f32,
    pub layer: Layer,
}

impl Motion {
    pub fn new(rng: &mut impl Rng, index: usize, count: usize, layer: Layer) -> Self {
        let mut motion = Self::with_layer(rng, layer);
        motion.y = (index as f32 + rng.gen_range(0.0..1.0)) / count as f32;
        motion
    }

    pub fn respawn(rng: &mut impl Rng) -> Self {
        let layer = if rng.gen_bool(0.4) {
            Layer::BehindPhotos
        } else {
            Layer::OverPhotos
        };
        Self::with_layer(rng, layer)
    }

    pub fn respawn_behind(rng: &mut impl Rng) -> Self {
        Self::with_layer(rng, Layer::BehindPhotos)
    }

    fn with_layer(rng: &mut impl Rng, layer: Layer) -> Self {
        let behind = layer == Layer::BehindPhotos;
        Self {
            x: rng.gen_range(0.04..0.96),
            y: rng.gen_range(1.12..1.5),
            size: if behind {
                rng.gen_range(0.18..0.28)
            } else {
                rng.gen_range(0.12..0.21)
            },
            vx: if behind {
                rng.gen_range(-0.003..0.003)
            } else {
                rng.gen_range(-0.006..0.006)
            },
            vy: if behind {
                -rng.gen_range(0.009..0.018)
            } else {
                -rng.gen_range(0.018..0.038)
            },
            angle: rng.gen_range(-0.6..0.6),
            spin: rng.gen_range(-0.12..0.12),
            opacity: if behind {
                rng.gen_range(0.52..0.7)
            } else {
                rng.gen_range(0.25..0.35)
            },
            layer,
        }
    }

    pub fn advance(&mut self, seconds: f32) {
        self.x += self.vx * seconds;
        self.y += self.vy * seconds;
        self.angle += self.spin * seconds;
    }

    pub fn rect(&self, screen_w: f32, screen_h: f32) -> [f32; 4] {
        let size = self.size * screen_w.min(screen_h);
        [
            self.x * screen_w - size * 0.5,
            self.y * screen_h - size * 0.5,
            size,
            size,
        ]
    }

    pub fn offscreen(&self, screen_w: f32, screen_h: f32) -> bool {
        let size = self.size * screen_w.min(screen_h);
        self.y * screen_h < -size
            || self.x * screen_w < -size
            || self.x * screen_w > screen_w + size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use std::collections::HashSet;

    #[test]
    fn sprites_have_soft_visible_shapes_and_clear_backgrounds() {
        for kind in ShapeKind::ALL {
            let pixels = rasterize(kind, PASTELS[0]);
            assert_eq!(pixels.len(), (SPRITE_SIZE * SPRITE_SIZE * 4) as usize);
            assert_eq!(pixels[3], 0);
            assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 255));
        }
        let ring = rasterize(ShapeKind::Donut, PASTELS[0]);
        let center = ((SPRITE_SIZE / 2 * SPRITE_SIZE + SPRITE_SIZE / 2) * 4 + 3) as usize;
        assert_eq!(ring[center], 0);
    }

    #[test]
    fn each_active_shape_can_have_a_distinct_pastel() {
        assert!(PASTELS.len() >= FLOATING_SHAPE_COUNT);
        assert_eq!(
            PASTELS.into_iter().collect::<HashSet<_>>().len(),
            PASTELS.len()
        );
    }

    #[test]
    fn rear_shapes_drift_more_slowly_than_front_shapes() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        for _ in 0..100 {
            let rear = Motion::with_layer(&mut rng, Layer::BehindPhotos);
            let front = Motion::with_layer(&mut rng, Layer::OverPhotos);
            assert!(rear.vy.abs() < front.vy.abs());
        }
    }
}
