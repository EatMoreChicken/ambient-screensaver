mod clock;
mod fonts;
mod gnome;
mod photos;
mod polaroid;
mod shapes;

use chrono::{DateTime, Local, NaiveDate};
use image::{metadata::Orientation, DynamicImage, ImageDecoder, ImageReader};
use rand::{seq::SliceRandom, Rng};
use std::{
    collections::VecDeque,
    env,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Fullscreen, Window, WindowBuilder},
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
    card_uv: [f32; 2],
    arch_height: f32,
    card_aspect: f32,
    mask_bits: f32,
    corner_radius: f32,
}

fn is_vertical_card(rect: &[f32; 4]) -> bool {
    rect[3] > rect[2]
}

const TOP_LEFT: u8 = 1;
const TOP_RIGHT: u8 = 2;
const BOTTOM_RIGHT: u8 = 4;
const BOTTOM_LEFT: u8 = 8;
const POLAROID: u8 = 32;
const STAMP: u8 = 64;
const PAPER_SHADOW: u8 = 128;
const STAMP_COLOR_COUNT: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PhotoMask {
    None,
    Arch,
    LargeCorners(u8),
    Polaroid,
    Stamp(u8),
    PaperShadow,
}

fn is_paper_mask(mask: PhotoMask) -> bool {
    matches!(mask, PhotoMask::Polaroid | PhotoMask::Stamp(_))
}

fn choose_photo_mask(rng: &mut impl Rng, quiet: bool, paper_cooldown: usize) -> PhotoMask {
    let mask = if quiet && rng.gen_bool(0.5) {
        PhotoMask::None
    } else {
        random_photo_mask(rng)
    };
    if paper_cooldown > 0 && is_paper_mask(mask) {
        PhotoMask::None
    } else {
        mask
    }
}

fn random_photo_mask(rng: &mut impl Rng) -> PhotoMask {
    match rng.gen_range(0..24) {
        0..=3 => PhotoMask::Arch,
        4 => PhotoMask::LargeCorners(TOP_LEFT | BOTTOM_RIGHT),
        5 => PhotoMask::LargeCorners(TOP_RIGHT | BOTTOM_LEFT),
        6 => PhotoMask::LargeCorners(1 << rng.gen_range(0..4)),
        7 => PhotoMask::LargeCorners(TOP_LEFT | TOP_RIGHT | BOTTOM_RIGHT | BOTTOM_LEFT),
        8 | 9 => PhotoMask::Stamp(rng.gen_range(0..STAMP_COLOR_COUNT)),
        10 | 11 => PhotoMask::Polaroid,
        _ => PhotoMask::None,
    }
}

fn visible_mask(mask: PhotoMask, rect: &[f32; 4]) -> PhotoMask {
    if matches!(mask, PhotoMask::Arch | PhotoMask::Polaroid) && !is_vertical_card(rect) {
        PhotoMask::None
    } else {
        mask
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Scroll,
    Slides,
}

fn parse_style(value: &str) -> Result<Style, String> {
    match value {
        "scroll" => Ok(Style::Scroll),
        "slides" => Ok(Style::Slides),
        _ => Err(format!("invalid style: {value}; use scroll or slides")),
    }
}

fn default_duration(style: Style) -> Duration {
    match style {
        Style::Scroll => Duration::from_secs(48),
        Style::Slides => Duration::from_secs(8),
    }
}

const STARTUP_FADE: Duration = Duration::from_millis(1500);

fn startup_opacity(elapsed: Duration) -> f32 {
    let progress = (elapsed.as_secs_f32() / STARTUP_FADE.as_secs_f32()).clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

fn parse_scroll_speed(value: &str) -> Result<Duration, String> {
    let screens_per_minute: f64 = value
        .parse()
        .map_err(|_| format!("invalid scroll speed: {value}"))?;
    if !screens_per_minute.is_finite() || screens_per_minute <= 0.0 {
        return Err("--scroll-speed must be a positive number".into());
    }
    let seconds_per_screen = 60.0 / screens_per_minute;
    if !(0.01..=86_400.0).contains(&seconds_per_screen) {
        return Err("--scroll-speed is outside the supported range".into());
    }
    Ok(Duration::from_secs_f64(seconds_per_screen))
}

struct Settings {
    directories: Vec<PathBuf>,
    duration: Duration,
    transition: Duration,
    background_color: [u8; 3],
    style: Style,
    windowed: bool,
    quiet: bool,
    no_shapes: bool,
}

fn parse_background_color(value: &str) -> Result<[u8; 3], String> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("invalid background color: {value}; use #RRGGBB"));
    }
    Ok([
        u8::from_str_radix(&hex[0..2], 16).unwrap(),
        u8::from_str_radix(&hex[2..4], 16).unwrap(),
        u8::from_str_radix(&hex[4..6], 16).unwrap(),
    ])
}

fn background_clear_color(rgb: [u8; 3], srgb_surface: bool) -> wgpu::Color {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if !srgb_surface {
            value
        } else if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    wgpu::Color {
        r: channel(rgb[0]),
        g: channel(rgb[1]),
        b: channel(rgb[2]),
        a: 1.0,
    }
}

fn settings() -> Result<Settings, String> {
    let mut args = env::args().skip(1);
    let mut duration_given = false;
    let mut scroll_speed = None;
    let mut settings = Settings {
        directories: Vec::new(),
        duration: default_duration(Style::Scroll),
        transition: Duration::from_millis(1500),
        background_color: [0xF5, 0xF0, 0xE6],
        style: Style::Scroll,
        windowed: false,
        quiet: false,
        no_shapes: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Usage: ambient-screensaver [--windowed] [--quiet] [--no-shapes] [--style scroll|slides] [--scroll-speed SCREENS_PER_MINUTE] [--duration SECONDS] [--transition SECONDS] [--background-color '#RRGGBB'] PHOTO_DIR [PHOTO_DIR ...]");
                println!("GNOME setup: ambient-screensaver gnome install [--idle-seconds N] PHOTO_DIR [PHOTO_DIR ...]");
                println!("Font licenses: ambient-screensaver --font-licenses");
                std::process::exit(0);
            }
            "--windowed" => settings.windowed = true,
            "--quiet" => settings.quiet = true,
            "--no-shapes" => settings.no_shapes = true,
            "--style" => {
                let value = args.next().ok_or("--style needs scroll or slides")?;
                settings.style = parse_style(&value)?;
            }
            "--scroll-speed" => {
                let value = args.next().ok_or("--scroll-speed needs a number")?;
                scroll_speed = Some(parse_scroll_speed(&value)?);
            }
            "--background-color" => {
                let value = args.next().ok_or("--background-color needs a hex color")?;
                settings.background_color = parse_background_color(&value)?;
            }
            "--duration" | "--transition" => {
                let value = args
                    .next()
                    .ok_or(format!("{arg} needs a number of seconds"))?;
                let seconds: f64 = value
                    .parse()
                    .map_err(|_| format!("invalid value for {arg}: {value}"))?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return Err(format!("{arg} must be positive"));
                }
                if arg == "--duration" {
                    settings.duration = Duration::from_secs_f64(seconds);
                    duration_given = true;
                } else {
                    settings.transition = Duration::from_secs_f64(seconds);
                }
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => settings.directories.push(PathBuf::from(arg)),
        }
    }
    if settings.directories.is_empty() {
        return Err("provide at least one photo directory; see --help".into());
    }
    if let Some(speed_duration) = scroll_speed {
        if settings.style != Style::Scroll {
            return Err("--scroll-speed is only available with --style scroll".into());
        }
        if duration_given {
            return Err("use either --scroll-speed or --duration, not both".into());
        }
        settings.duration = speed_duration;
    } else if !duration_given {
        settings.duration = default_duration(settings.style);
    }
    Ok(settings)
}

struct DecodedPhoto {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    date: Option<NaiveDate>,
}

fn exif_capture_date(bytes: Vec<u8>) -> Option<NaiveDate> {
    let exif = exif::Reader::new().read_raw(bytes).ok()?;
    let field = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)?;
    let exif::Value::Ascii(values) = &field.value else {
        return None;
    };
    let text = std::str::from_utf8(values.first()?.get(..10)?).ok()?;
    NaiveDate::parse_from_str(text, "%Y:%m:%d").ok()
}

fn file_modified_date(path: &Path) -> Option<NaiveDate> {
    let modified = path.metadata().ok()?.modified().ok()?;
    Some(DateTime::<Local>::from(modified).date_naive())
}

fn open_oriented(
    path: &Path,
) -> Result<(DynamicImage, Option<NaiveDate>), Box<dyn std::error::Error + Send + Sync>> {
    let mut decoder = ImageReader::open(path)?.into_decoder()?;
    let capture_date = decoder
        .exif_metadata()
        .ok()
        .flatten()
        .and_then(exif_capture_date);
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok((image, capture_date.or_else(|| file_modified_date(path))))
}

fn start_loader(paths: Vec<PathBuf>) -> mpsc::Receiver<DecodedPhoto> {
    let (sender, receiver) = mpsc::sync_channel(3);
    std::thread::spawn(move || {
        let count = paths.len();
        let mut bag = photos::ShuffleBag::new(paths);
        let mut failures = 0;
        while let Some(path) = bag.next() {
            match open_oriented(&path) {
                Ok((image, date)) => {
                    failures = 0;
                    let image = if image.width() > 2560 || image.height() > 2560 {
                        // Keep large-photo previews responsive in unoptimized builds.
                        if cfg!(debug_assertions) {
                            image.thumbnail(2560, 2560)
                        } else {
                            image.resize(2560, 2560, image::imageops::FilterType::Lanczos3)
                        }
                    } else {
                        image
                    }
                    .to_rgba8();
                    if sender
                        .send(DecodedPhoto {
                            width: image.width(),
                            height: image.height(),
                            pixels: image.into_raw(),
                            date,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    eprintln!("Skipping {}: {error}", path.display());
                    failures += 1;
                    if failures >= count {
                        break;
                    }
                }
            }
        }
    });
    receiver
}

struct Photo {
    width: u32,
    height: u32,
    mask: PhotoMask,
    date_label: Option<Box<Photo>>,
    bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
}

struct FloatingShape {
    photo: Photo,
    motion: shapes::Motion,
}

fn layout_mode(slide_number: u64, photo_count: usize) -> usize {
    (slide_number % photo_count.clamp(1, 3) as u64) as usize
}

fn layout_photo_count(mode: usize) -> usize {
    mode + 1
}

fn next_layout_ready(slide_number: u64, photo_count: usize, loaded_count: usize) -> bool {
    let next_mode = layout_mode(slide_number + 1, photo_count);
    photo_count > 1 && loaded_count > layout_photo_count(next_mode)
}

fn layout_rects(mode: usize, width: f32, height: f32) -> Vec<[f32; 4]> {
    match mode {
        0 => vec![[width * 0.035, height * 0.04, width * 0.93, height * 0.92]],
        1 => vec![
            [width * 0.045, height * 0.08, width * 0.47, height * 0.84],
            [width * 0.535, height * 0.08, width * 0.42, height * 0.84],
        ],
        2 => vec![
            [width * 0.04, height * 0.06, width * 0.57, height * 0.88],
            [width * 0.64, height * 0.06, width * 0.32, height * 0.42],
            [width * 0.64, height * 0.52, width * 0.32, height * 0.42],
        ],
        _ => unreachable!("invalid layout mode"),
    }
}

struct ScrollGroup {
    first_photo: usize,
    width: f32,
    rects: Vec<[f32; 4]>,
}

struct ScrollHistoryGroup {
    pattern: usize,
    photos: Vec<Photo>,
}

const SCROLL_PAUSE: Duration = Duration::from_secs(3);
const SCROLL_HISTORY_PHOTOS: usize = 8;
const SCROLL_KEY_SPEED: f32 = 0.65;
const SCROLL_HORIZONTAL_GAP: f32 = 0.028;

fn scroll_motion(
    delta: f32,
    width: f32,
    duration: f32,
    left_held: bool,
    right_held: bool,
    paused: bool,
) -> f32 {
    match (left_held, right_held) {
        (true, false) => delta * width * SCROLL_KEY_SPEED,
        (false, true) => -delta * width * SCROLL_KEY_SPEED,
        _ if paused => 0.0,
        _ => delta * width / duration,
    }
}

fn scroll_group_lefts(head_x: f32, groups: &[ScrollGroup], gap: f32) -> Vec<f32> {
    let mut x = head_x;
    groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            if index > 0 {
                x -= gap + group.width;
            }
            x
        })
        .collect()
}

struct ScrollRows {
    top: f32,
    full: f32,
    half: f32,
    bottom: f32,
}

impl ScrollRows {
    fn new(height: f32) -> Self {
        let top = height * 0.07;
        let full = height * 0.86;
        let gap = height * 0.04;
        let half = (full - gap) / 2.0;
        Self {
            top,
            full,
            half,
            bottom: top + half + gap,
        }
    }
}

fn portrait_card_size(width: f32, height: f32) -> (f32, f32) {
    let card_height = ScrollRows::new(height).full;
    let card_width = (card_height * 2.0 / 3.0).clamp(width * 0.32, width * 0.84);
    (card_width, card_height)
}

fn card_corner_radius(screen_w: f32, screen_h: f32, card_w: f32, card_h: f32) -> f32 {
    (portrait_card_size(screen_w, screen_h).0 * 0.18).min(card_w.min(card_h) * 0.28)
}

fn polaroid_photo_bounds(card_aspect: f32) -> [f32; 4] {
    let side_inset = 0.055;
    [side_inset, side_inset * card_aspect, 1.0 - side_inset, 0.78]
}

fn stamp_photo_bounds(card_w: f32, card_h: f32, radius_px: f32) -> [f32; 4] {
    let inset = radius_px * 0.55;
    [
        inset / card_w,
        inset / card_h,
        1.0 - inset / card_w,
        1.0 - inset / card_h,
    ]
}

const SCROLL_PATTERN_COUNT: usize = 8;

fn scroll_pattern_photo_count(pattern: usize) -> usize {
    match pattern {
        0 => 1,
        1 | 3 | 4 => 2,
        2 | 5 | 7 => 3,
        6 => 4,
        _ => unreachable!("invalid scroll pattern"),
    }
}

fn scroll_photo_limit(patterns: &VecDeque<usize>, minimum: usize) -> usize {
    let mut total = 0;
    for &pattern in patterns {
        total += scroll_pattern_photo_count(pattern);
        if total >= minimum {
            return total;
        }
    }
    total
}

struct LayoutBag {
    available: Vec<usize>,
    remaining: Vec<usize>,
    last: Option<usize>,
}

impl LayoutBag {
    fn new(photo_count: usize) -> Self {
        let available = (0..SCROLL_PATTERN_COUNT)
            .filter(|&pattern| scroll_pattern_photo_count(pattern) <= photo_count)
            .collect();
        Self {
            available,
            remaining: Vec::new(),
            last: None,
        }
    }

    fn next(&mut self) -> usize {
        if self.remaining.is_empty() {
            self.remaining = self.available.clone();
            self.remaining.shuffle(&mut rand::thread_rng());
            if self.remaining.len() > 1 && self.remaining.last() == self.last.as_ref() {
                let last = self.remaining.len() - 1;
                self.remaining.swap(last, 0);
            }
        }
        let pattern = self.remaining.pop().expect("at least one scroll pattern");
        self.last = Some(pattern);
        pattern
    }
}

fn scroll_layout(mode: usize, first_aspect: f32, width: f32, height: f32) -> (f32, Vec<[f32; 4]>) {
    let rows = ScrollRows::new(height);
    match mode {
        0 => {
            let (portrait_width, card_height) = portrait_card_size(width, height);
            let card_width = if first_aspect < 1.0 {
                portrait_width
            } else {
                (card_height * first_aspect).clamp(width * 0.32, width * 0.84)
            };
            (card_width, vec![[0.0, rows.top, card_width, card_height]])
        }
        1 => {
            let group_width = width * 0.46;
            (
                group_width,
                vec![
                    [0.0, rows.top, group_width, rows.half],
                    [0.0, rows.bottom, group_width, rows.half],
                ],
            )
        }
        2 => {
            let (left_width, card_height) = portrait_card_size(width, height);
            let right_x = left_width + width * SCROLL_HORIZONTAL_GAP;
            let right_width = width * 0.33;
            (
                right_x + right_width,
                vec![
                    [0.0, rows.top, left_width, card_height],
                    [right_x, rows.top, right_width, rows.half],
                    [right_x, rows.bottom, right_width, rows.half],
                ],
            )
        }
        3 => {
            let (card_width, card_height) = portrait_card_size(width, height);
            let second_x = card_width + width * SCROLL_HORIZONTAL_GAP;
            (
                second_x + card_width,
                vec![
                    [0.0, rows.top, card_width, card_height],
                    [second_x, rows.top, card_width, card_height],
                ],
            )
        }
        4 => {
            let group_width = width * 0.62;
            (
                group_width,
                vec![
                    [0.0, rows.top, group_width, rows.half],
                    [0.0, rows.bottom, group_width, rows.half],
                ],
            )
        }
        5 => {
            let (card_width, card_height) = portrait_card_size(width, height);
            let step = card_width + width * SCROLL_HORIZONTAL_GAP;
            (
                step * 2.0 + card_width,
                vec![
                    [0.0, rows.top, card_width, card_height],
                    [step, rows.top, card_width, card_height],
                    [step * 2.0, rows.top, card_width, card_height],
                ],
            )
        }
        6 => {
            let card_width = width * 0.34;
            let second_x = card_width + width * SCROLL_HORIZONTAL_GAP;
            (
                second_x + card_width,
                vec![
                    [0.0, rows.top, card_width, rows.half],
                    [second_x, rows.top, card_width, rows.half],
                    [0.0, rows.bottom, card_width, rows.half],
                    [second_x, rows.bottom, card_width, rows.half],
                ],
            )
        }
        7 => {
            let (portrait_width, portrait_height) = portrait_card_size(width, height);
            let stack_width = width * 0.33;
            let tall_x = stack_width + width * SCROLL_HORIZONTAL_GAP;
            (
                tall_x + portrait_width,
                vec![
                    [0.0, rows.top, stack_width, rows.half],
                    [0.0, rows.bottom, stack_width, rows.half],
                    [tall_x, rows.top, portrait_width, portrait_height],
                ],
            )
        }
        _ => unreachable!("invalid scroll layout mode"),
    }
}

struct SceneMotion {
    alpha: f32,
    shift_x: f32,
    drift: f32,
}

fn scene_cards(
    photos: &VecDeque<Photo>,
    first: usize,
    mode: usize,
    width: f32,
    height: f32,
    motion: SceneMotion,
) -> Vec<(&Photo, [f32; 4], f32)> {
    layout_rects(mode, width, height)
        .into_iter()
        .enumerate()
        .map(|(slot, bounds)| {
            let photo = &photos[first + slot];
            let mut rect = if mode == 0 {
                fitted_rect(photo, bounds)
            } else {
                bounds
            };
            rect[0] += motion.shift_x;
            if mode == 0 {
                rect[0] += width * 0.012 * motion.drift;
                rect[1] -= height * 0.008 * motion.drift;
            }
            (photo, rect, motion.alpha)
        })
        .collect()
}

struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    background_color: wgpu::Color,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vertices: wgpu::Buffer,
    clock: clock::Clock,
    date_stamp: polaroid::DateStamp,
    clock_photo: Option<Photo>,
    clock_rect: [f32; 4],
    clock_key: String,
    clock_screen_size: (u32, u32),
    photos: VecDeque<Photo>,
    floating_shapes: Vec<FloatingShape>,
    quiet: bool,
    no_shapes: bool,
    paper_cooldown: usize,
    shapes_last_tick: Instant,
    receiver: mpsc::Receiver<DecodedPhoto>,
    slide_started: Instant,
    startup_fade_started: Option<Instant>,
    transition_started: Option<Instant>,
    duration: Duration,
    transition: Duration,
    slide_number: u64,
    photo_count: usize,
    style: Style,
    scroll_head_x: Option<f32>,
    scroll_last_tick: Instant,
    scroll_pause_until: Option<Instant>,
    scroll_left_held: bool,
    scroll_right_held: bool,
    scroll_patterns: VecDeque<usize>,
    scroll_history: VecDeque<ScrollHistoryGroup>,
    layout_bag: LayoutBag,
}

impl Renderer {
    async fn new(
        window: Arc<Window>,
        receiver: mpsc::Receiver<DecodedPhoto>,
        settings: &Settings,
        photo_count: usize,
    ) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|error| error.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no compatible graphics adapter found")?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ambient device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|error| error.to_string())?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let background_color = background_clear_color(settings.background_color, format.is_srgb());
        surface.configure(&device, &config);
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photo bind layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photo shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("photo pipeline layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("photo pipeline"), layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_main", buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32, 3 => Float32x2, 4 => Float32, 5 => Float32, 6 => Float32, 7 => Float32],
            }] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_main", targets: &[Some(wgpu::ColorTargetState {
                format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL,
            })] }),
            primitive: wgpu::PrimitiveState::default(), depth_stencil: None,
            multisample: wgpu::MultisampleState::default(), multiview: None,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("card vertices"),
            size: (6 * 64 * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            background_color,
            pipeline,
            bind_layout,
            sampler,
            vertices,
            clock: clock::Clock::new()?,
            date_stamp: polaroid::DateStamp::new()?,
            clock_photo: None,
            clock_rect: [0.0; 4],
            clock_key: String::new(),
            clock_screen_size: (0, 0),
            photos: VecDeque::new(),
            floating_shapes: Vec::new(),
            quiet: settings.quiet,
            no_shapes: settings.no_shapes,
            paper_cooldown: 0,
            shapes_last_tick: Instant::now(),
            receiver,
            slide_started: Instant::now(),
            startup_fade_started: None,
            transition_started: None,
            duration: settings.duration,
            transition: settings.transition,
            slide_number: 0,
            photo_count,
            style: settings.style,
            scroll_head_x: None,
            scroll_last_tick: Instant::now(),
            scroll_pause_until: None,
            scroll_left_held: false,
            scroll_right_held: false,
            scroll_patterns: VecDeque::new(),
            scroll_history: VecDeque::new(),
            layout_bag: LayoutBag::new(photo_count),
        };
        if !renderer.no_shapes {
            renderer.init_shapes();
        }
        Ok(renderer)
    }

    fn init_shapes(&mut self) {
        let mut rng = rand::thread_rng();
        let mut kinds = shapes::ShapeKind::ALL;
        kinds.shuffle(&mut rng);
        let mut colors = shapes::PASTELS;
        colors.shuffle(&mut rng);
        let mut layers = [
            shapes::Layer::BehindPhotos,
            shapes::Layer::BehindPhotos,
            shapes::Layer::BehindPhotos,
            shapes::Layer::BehindPhotos,
            shapes::Layer::OverPhotos,
            shapes::Layer::OverPhotos,
        ];
        layers.shuffle(&mut rng);
        assert_eq!(layers.len(), shapes::FLOATING_SHAPE_COUNT);
        let count = if self.quiet {
            3
        } else {
            shapes::FLOATING_SHAPE_COUNT
        };
        for index in 0..count {
            let pixels = shapes::rasterize(kinds[index % kinds.len()], colors[index]);
            let photo = self.upload(
                DecodedPhoto {
                    width: shapes::SPRITE_SIZE,
                    height: shapes::SPRITE_SIZE,
                    pixels,
                    date: None,
                },
                PhotoMask::None,
            );
            self.floating_shapes.push(FloatingShape {
                photo,
                motion: shapes::Motion::new(
                    &mut rng,
                    index,
                    count,
                    if self.quiet {
                        shapes::Layer::BehindPhotos
                    } else {
                        layers[index]
                    },
                ),
            });
        }
    }

    fn update_shapes(&mut self) {
        let now = Instant::now();
        let delta = now
            .duration_since(self.shapes_last_tick)
            .as_secs_f32()
            .min(0.1);
        self.shapes_last_tick = now;
        let mut rng = rand::thread_rng();
        for shape in &mut self.floating_shapes {
            shape
                .motion
                .advance(if self.quiet { delta * 0.5 } else { delta });
            if shape
                .motion
                .offscreen(self.config.width as f32, self.config.height as f32)
            {
                shape.motion = if self.quiet {
                    shapes::Motion::respawn_behind(&mut rng)
                } else {
                    shapes::Motion::respawn(&mut rng)
                };
            }
        }
    }

    fn upload(&self, decoded: DecodedPhoto, mask: PhotoMask) -> Photo {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("photo"),
            size: wgpu::Extent3d {
                width: decoded.width,
                height: decoded.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &decoded.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * decoded.width),
                rows_per_image: Some(decoded.height),
            },
            wgpu::Extent3d {
                width: decoded.width,
                height: decoded.height,
                depth_or_array_layers: 1,
            },
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photo bind group"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Photo {
            width: decoded.width,
            height: decoded.height,
            mask,
            date_label: None,
            bind_group,
            _texture: texture,
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.scroll_head_x = self
            .scroll_head_x
            .map(|x| x * size.width as f32 / self.config.width as f32);
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    fn update(&mut self) -> bool {
        if self.style == Style::Scroll {
            while self.scroll_patterns.len() < 10 {
                self.scroll_patterns.push_back(self.layout_bag.next());
            }
        }
        let photo_limit = match self.style {
            Style::Scroll => scroll_photo_limit(&self.scroll_patterns, 10),
            Style::Slides => 4,
        };
        while self.photos.len() < photo_limit {
            match self.receiver.try_recv() {
                Ok(decoded) => {
                    if self.photos.is_empty() {
                        self.slide_started = Instant::now();
                    }
                    let mut mask = if self.style == Style::Scroll {
                        choose_photo_mask(&mut rand::thread_rng(), self.quiet, self.paper_cooldown)
                    } else {
                        PhotoMask::None
                    };
                    let date_label = if mask == PhotoMask::Polaroid {
                        decoded.date.map(|date| {
                            let image = self.date_stamp.draw(date);
                            Box::new(self.upload(
                                DecodedPhoto {
                                    pixels: image.pixels,
                                    width: image.width,
                                    height: image.height,
                                    date: None,
                                },
                                PhotoMask::None,
                            ))
                        })
                    } else {
                        None
                    };
                    if date_label.is_none() && mask == PhotoMask::Polaroid {
                        mask = PhotoMask::None;
                    }
                    self.paper_cooldown = if is_paper_mask(mask) {
                        3
                    } else {
                        self.paper_cooldown.saturating_sub(1)
                    };
                    let mut photo = self.upload(decoded, mask);
                    photo.date_label = date_label;
                    self.photos.push_back(photo);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return !self.photos.is_empty(),
            }
        }
        match self.style {
            Style::Scroll => self.update_scroll(),
            Style::Slides => self.update_slides(),
        }
        self.update_shapes();
        let first_scene_ready = match self.style {
            Style::Scroll => self.scroll_head_x.is_some(),
            Style::Slides => self.photos.len() >= layout_photo_count(0),
        };
        if first_scene_ready && self.startup_fade_started.is_none() {
            eprintln!("Photo display ready");
            self.startup_fade_started = Some(Instant::now());
        }
        true
    }

    fn update_scroll(&mut self) {
        let now = Instant::now();
        let delta = now
            .duration_since(self.scroll_last_tick)
            .as_secs_f32()
            .min(0.1);
        self.scroll_last_tick = now;
        let arrow_held = self.scroll_left_held || self.scroll_right_held;
        let paused = arrow_held || self.scroll_pause_until.is_some_and(|until| now < until);
        let width = self.config.width as f32;
        let groups = self.scroll_groups(width, self.config.height as f32);
        if groups.len() < 2 {
            return;
        }
        let gap = width * SCROLL_HORIZONTAL_GAP;
        if self.scroll_head_x.is_none() {
            let x = width - groups[0].width;
            if scroll_group_lefts(x, &groups, gap)
                .last()
                .copied()
                .unwrap_or(width)
                > 0.0
            {
                return;
            }
            self.scroll_head_x = Some(x);
        }
        let motion = scroll_motion(
            delta,
            width,
            self.duration.as_secs_f32(),
            self.scroll_left_held,
            self.scroll_right_held,
            paused,
        );
        if motion < 0.0 {
            self.rewind_scroll(-motion);
        } else if motion > 0.0 {
            self.advance_scroll(motion);
        }
    }

    fn set_scroll_arrow(&mut self, key: NamedKey, pressed: bool) {
        self.scroll_pause_until = Some(Instant::now() + SCROLL_PAUSE);
        match key {
            NamedKey::ArrowLeft => self.scroll_left_held = pressed,
            NamedKey::ArrowRight => self.scroll_right_held = pressed,
            _ => unreachable!("only arrow keys move the scroll"),
        }
    }

    fn release_scroll_arrows(&mut self) {
        if self.scroll_left_held || self.scroll_right_held {
            self.scroll_pause_until = Some(Instant::now() + SCROLL_PAUSE);
            self.scroll_left_held = false;
            self.scroll_right_held = false;
        }
    }

    fn advance_scroll(&mut self, distance: f32) {
        let width = self.config.width as f32;
        let groups = self.scroll_groups(width, self.config.height as f32);
        let Some(head_x) = self.scroll_head_x else {
            return;
        };
        if groups.len() < 2 {
            return;
        }
        let gap = width * SCROLL_HORIZONTAL_GAP;
        let leftmost = scroll_group_lefts(head_x, &groups, gap)
            .last()
            .copied()
            .unwrap_or(0.0);
        let mut new_head_x = head_x + distance.min((-leftmost).max(0.0));
        let mut group_index = 0;
        while group_index + 1 < groups.len() && new_head_x >= width {
            let pattern = self.scroll_patterns.pop_front().unwrap();
            let photos = (0..scroll_pattern_photo_count(pattern))
                .map(|_| self.photos.pop_front().unwrap())
                .collect();
            self.scroll_history
                .push_back(ScrollHistoryGroup { pattern, photos });
            while self.scroll_history.len() > 1
                && self
                    .scroll_history
                    .iter()
                    .map(|group| group.photos.len())
                    .sum::<usize>()
                    > SCROLL_HISTORY_PHOTOS
            {
                self.scroll_history.pop_front();
            }
            self.slide_number += 1;
            group_index += 1;
            new_head_x -= gap + groups[group_index].width;
        }
        self.scroll_head_x = Some(new_head_x);
    }

    fn rewind_scroll(&mut self, distance: f32) {
        let width = self.config.width as f32;
        let height = self.config.height as f32;
        let Some(mut head_x) = self.scroll_head_x.map(|x| x - distance) else {
            return;
        };
        let gap = width * SCROLL_HORIZONTAL_GAP;
        loop {
            let groups = self.scroll_groups(width, height);
            let Some(first) = groups.first() else {
                return;
            };
            if head_x + first.width >= width {
                break;
            }
            let Some(mut previous) = self.scroll_history.pop_back() else {
                head_x = width - first.width;
                break;
            };
            head_x += gap + first.width;
            self.scroll_patterns.push_front(previous.pattern);
            while let Some(photo) = previous.photos.pop() {
                self.photos.push_front(photo);
            }
            self.slide_number = self.slide_number.saturating_sub(1);
        }
        self.scroll_head_x = Some(head_x);
    }

    fn scroll_groups(&self, width: f32, height: f32) -> Vec<ScrollGroup> {
        let mut first_photo = 0;
        let mut groups = Vec::new();
        for &pattern in &self.scroll_patterns {
            let count = scroll_pattern_photo_count(pattern);
            if first_photo + count > self.photos.len() {
                break;
            }
            let first = &self.photos[first_photo];
            let aspect = first.width as f32 / first.height as f32;
            let (group_width, rects) = scroll_layout(pattern, aspect, width, height);
            groups.push(ScrollGroup {
                first_photo,
                width: group_width,
                rects,
            });
            first_photo += count;
        }
        groups
    }

    fn update_slides(&mut self) {
        if self.transition_started.is_none()
            && self.slide_started.elapsed() >= self.duration
            && next_layout_ready(self.slide_number, self.photo_count, self.photos.len())
        {
            self.transition_started = Some(Instant::now());
        }
        if let Some(started) = self.transition_started {
            if started.elapsed() >= self.transition {
                self.photos.pop_front();
                self.slide_started = Instant::now();
                self.transition_started = None;
                self.slide_number += 1;
            }
        }
    }

    fn scroll_cards(&self, width: f32, height: f32) -> Vec<(&Photo, [f32; 4], f32)> {
        let Some(head_x) = self.scroll_head_x else {
            return Vec::new();
        };
        let groups = self.scroll_groups(width, height);
        let lefts = scroll_group_lefts(head_x, &groups, width * SCROLL_HORIZONTAL_GAP);
        let mut cards = Vec::new();
        for (group, left) in groups.iter().zip(lefts) {
            for (slot, local_rect) in group.rects.iter().enumerate() {
                let mut rect = *local_rect;
                rect[0] += left;
                if rect[0] + rect[2] > 0.0 && rect[0] < width {
                    cards.push((&self.photos[group.first_photo + slot], rect, 1.0));
                }
            }
        }
        cards
    }

    fn slides_cards(&self, width: f32, height: f32) -> Vec<(&Photo, [f32; 4], f32)> {
        let elapsed = self.slide_started.elapsed();
        let progress = self
            .transition_started
            .map(|started| {
                (started.elapsed().as_secs_f32() / self.transition.as_secs_f32()).clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let mode = layout_mode(self.slide_number, self.photo_count);
        let drift = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);
        let mut cards = if self.photos.len() >= layout_photo_count(mode) {
            scene_cards(
                &self.photos,
                0,
                mode,
                width,
                height,
                SceneMotion {
                    alpha: 1.0 - eased,
                    shift_x: -width * 0.08 * eased,
                    drift,
                },
            )
        } else {
            Vec::new()
        };
        if progress > 0.0 {
            let next_mode = layout_mode(self.slide_number + 1, self.photo_count);
            cards.extend(scene_cards(
                &self.photos,
                1,
                next_mode,
                width,
                height,
                SceneMotion {
                    alpha: eased,
                    shift_x: width * 0.08 * (1.0 - eased),
                    drift: 0.0,
                },
            ));
        }
        cards
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let now = Local::now();
        let clock_key = now.format("%Y-%m-%d %H:%M").to_string();
        let screen_size = (self.config.width, self.config.height);
        if self.clock_key != clock_key || self.clock_screen_size != screen_size {
            let bottom_mat_fraction = match self.style {
                Style::Scroll => 0.07,
                Style::Slides => 0.04,
            };
            let image = self.clock.draw(&now, screen_size, bottom_mat_fraction);
            self.clock_rect = image.rect;
            self.clock_photo = Some(self.upload(
                DecodedPhoto {
                    pixels: image.pixels,
                    width: image.width,
                    height: image.height,
                    date: None,
                },
                PhotoMask::None,
            ));
            self.clock_key = clock_key;
            self.clock_screen_size = screen_size;
        }
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ambient frame"),
            });
        let width = self.config.width as f32;
        let height = self.config.height as f32;
        let mut cards = match self.style {
            Style::Scroll => self.scroll_cards(width, height),
            Style::Slides => self.slides_cards(width, height),
        };
        let startup_alpha = self
            .startup_fade_started
            .map(|started| startup_opacity(started.elapsed()))
            .unwrap_or(0.0);
        for (_, _, alpha) in &mut cards {
            *alpha *= startup_alpha;
        }
        let mut draws = Vec::with_capacity(64);
        for shape in self
            .floating_shapes
            .iter()
            .filter(|shape| shape.motion.layer == shapes::Layer::BehindPhotos)
        {
            draws.push((
                &shape.photo,
                floating_shape_vertices(shape, width, height, startup_alpha),
            ));
        }
        for (photo, rect, alpha) in &cards {
            let mask = if self.style == Style::Scroll {
                visible_mask(photo.mask, rect)
            } else {
                PhotoMask::None
            };
            if is_paper_mask(mask) {
                draws.push((
                    *photo,
                    paper_shadow_vertices(rect, *alpha, photo, width, height),
                ));
            }
            draws.push((
                *photo,
                card_vertices(rect, *alpha, mask, photo, width, height),
            ));
            if mask == PhotoMask::Polaroid {
                if let Some(label) = &photo.date_label {
                    draws.push((
                        label.as_ref(),
                        polaroid_date_vertices(rect, *alpha, label, width, height),
                    ));
                }
            }
        }
        for shape in self
            .floating_shapes
            .iter()
            .filter(|shape| shape.motion.layer == shapes::Layer::OverPhotos)
        {
            draws.push((
                &shape.photo,
                floating_shape_vertices(shape, width, height, startup_alpha),
            ));
        }
        if let Some(photo) = &self.clock_photo {
            draws.push((
                photo,
                card_vertices(&self.clock_rect, 1.0, PhotoMask::None, photo, width, height),
            ));
        }
        let stride = (6 * std::mem::size_of::<Vertex>()) as u64;
        for (index, (_, vertices)) in draws.iter().enumerate() {
            self.queue.write_buffer(
                &self.vertices,
                index as u64 * stride,
                bytemuck::cast_slice(vertices),
            );
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ambient cards"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.background_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (index, (photo, _)) in draws.iter().enumerate() {
                pass.set_bind_group(0, &photo.bind_group, &[]);
                pass.set_vertex_buffer(
                    0,
                    self.vertices
                        .slice(index as u64 * stride..(index as u64 + 1) * stride),
                );
                pass.draw(0..6, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

fn fitted_rect(photo: &Photo, bounds: [f32; 4]) -> [f32; 4] {
    let scale = (bounds[2] / photo.width as f32).min(bounds[3] / photo.height as f32);
    let w = photo.width as f32 * scale;
    let h = photo.height as f32 * scale;
    [
        bounds[0] + (bounds[2] - w) / 2.0,
        bounds[1] + (bounds[3] - h) / 2.0,
        w,
        h,
    ]
}

fn card_vertices(
    rect: &[f32; 4],
    alpha: f32,
    mask: PhotoMask,
    photo: &Photo,
    screen_w: f32,
    screen_h: f32,
) -> [Vertex; 6] {
    let [x, y, w, h] = *rect;
    let x0 = x / screen_w * 2.0 - 1.0;
    let x1 = (x + w) / screen_w * 2.0 - 1.0;
    let y0 = 1.0 - y / screen_h * 2.0;
    let y1 = 1.0 - (y + h) / screen_h * 2.0;
    let image_aspect = photo.width as f32 / photo.height as f32;
    let box_aspect = w / h;
    let radius_px = card_corner_radius(screen_w, screen_h, w, h);
    let (arch_height, mask_bits) = match mask {
        PhotoMask::None => (0.0, 0.0),
        PhotoMask::Arch => (w / (2.0 * h), 0.0),
        PhotoMask::LargeCorners(corners) => (0.0, f32::from(corners)),
        PhotoMask::Polaroid => (0.0, f32::from(POLAROID)),
        PhotoMask::Stamp(color) => (0.0, f32::from(STAMP) + f32::from(color) * 256.0),
        PhotoMask::PaperShadow => (0.0, f32::from(PAPER_SHADOW)),
    };
    let photo_bounds = match mask {
        PhotoMask::Polaroid => Some(polaroid_photo_bounds(box_aspect)),
        PhotoMask::Stamp(_) => Some(stamp_photo_bounds(w, h, radius_px)),
        _ => None,
    };
    let [photo_left, photo_top, photo_right, photo_bottom] =
        photo_bounds.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let photo_width = photo_right - photo_left;
    let photo_height = photo_bottom - photo_top;
    let photo_aspect = box_aspect * photo_width / photo_height;
    let (mut u0, mut u1, mut v0, mut v1) = if image_aspect > photo_aspect {
        let span = photo_aspect / image_aspect;
        ((1.0 - span) / 2.0, (1.0 + span) / 2.0, 0.0, 1.0)
    } else {
        let span = image_aspect / photo_aspect;
        (0.0, 1.0, (1.0 - span) / 2.0, (1.0 + span) / 2.0)
    };
    if photo_bounds.is_some() {
        let u_span = u1 - u0;
        let v_span = v1 - v0;
        u1 = u0 + (1.0 - photo_left) / photo_width * u_span;
        u0 -= photo_left / photo_width * u_span;
        v1 = v0 + (1.0 - photo_top) / photo_height * v_span;
        v0 -= photo_top / photo_height * v_span;
    }
    let vertex = |px, py, u, v, card_u, card_v| Vertex {
        position: [px, py],
        uv: [u, v],
        alpha,
        card_uv: [card_u, card_v],
        arch_height,
        card_aspect: box_aspect,
        mask_bits,
        corner_radius: radius_px / h,
    };
    [
        vertex(x0, y0, u0, v0, 0.0, 0.0),
        vertex(x0, y1, u0, v1, 0.0, 1.0),
        vertex(x1, y1, u1, v1, 1.0, 1.0),
        vertex(x0, y0, u0, v0, 0.0, 0.0),
        vertex(x1, y1, u1, v1, 1.0, 1.0),
        vertex(x1, y0, u1, v0, 1.0, 0.0),
    ]
}

fn paper_shadow_vertices(
    rect: &[f32; 4],
    alpha: f32,
    photo: &Photo,
    screen_w: f32,
    screen_h: f32,
) -> [Vertex; 6] {
    let spread = card_corner_radius(screen_w, screen_h, rect[2], rect[3]) * 0.22;
    let shadow_rect = [
        rect[0] - spread,
        rect[1] - spread,
        rect[2] + spread * 2.0,
        rect[3] + spread * 2.0,
    ];
    card_vertices(
        &shadow_rect,
        alpha,
        PhotoMask::PaperShadow,
        photo,
        screen_w,
        screen_h,
    )
}

fn polaroid_date_vertices(
    card: &[f32; 4],
    alpha: f32,
    label: &Photo,
    screen_w: f32,
    screen_h: f32,
) -> [Vertex; 6] {
    let label_width = card[2] * 0.64;
    let label_height = label_width * label.height as f32 / label.width as f32;
    let rect = [
        card[0] + (card[2] - label_width) / 2.0,
        card[1] + card[3] * 0.78 + (card[3] * 0.22 - label_height) / 2.0,
        label_width,
        label_height,
    ];
    let mut vertices = card_vertices(&rect, alpha, PhotoMask::None, label, screen_w, screen_h);
    rotate_vertices(
        &mut vertices,
        &rect,
        (-7.0_f32).to_radians(),
        screen_w,
        screen_h,
    );
    vertices
}

fn rotate_vertices(
    vertices: &mut [Vertex; 6],
    rect: &[f32; 4],
    angle: f32,
    screen_w: f32,
    screen_h: f32,
) {
    let center_x = rect[0] + rect[2] / 2.0;
    let center_y = rect[1] + rect[3] / 2.0;
    let (sin, cos) = angle.sin_cos();
    for vertex in vertices.iter_mut() {
        let x = (vertex.position[0] + 1.0) * screen_w / 2.0 - center_x;
        let y = (1.0 - vertex.position[1]) * screen_h / 2.0 - center_y;
        let rotated_x = center_x + x * cos - y * sin;
        let rotated_y = center_y + x * sin + y * cos;
        vertex.position = [
            rotated_x / screen_w * 2.0 - 1.0,
            1.0 - rotated_y / screen_h * 2.0,
        ];
    }
}

fn floating_shape_vertices(
    shape: &FloatingShape,
    screen_w: f32,
    screen_h: f32,
    startup_alpha: f32,
) -> [Vertex; 6] {
    let rect = shape.motion.rect(screen_w, screen_h);
    let mut vertices = card_vertices(
        &rect,
        shape.motion.opacity * startup_alpha,
        PhotoMask::None,
        &shape.photo,
        screen_w,
        screen_h,
    );
    for vertex in &mut vertices {
        vertex.corner_radius = 0.0;
    }
    rotate_vertices(&mut vertices, &rect, shape.motion.angle, screen_w, screen_h);
    vertices
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().nth(1).as_deref() == Some("--font-licenses") {
        fonts::print_licenses();
        return Ok(());
    }
    if env::args().nth(1).as_deref() == Some("gnome") {
        return gnome::run(env::args().skip(2).collect());
    }
    let settings = settings()?;
    let paths = photos::discover(&settings.directories);
    if paths.is_empty() {
        return Err("no supported photos found in the supplied directories".into());
    }
    eprintln!("Found {} photos", paths.len());
    let photo_count = paths.len();
    let receiver = start_loader(paths);
    let event_loop = EventLoop::new()?;
    let mut builder = WindowBuilder::new()
        .with_title("Ambient Photos")
        .with_inner_size(PhysicalSize::new(1280, 800));
    if !settings.windowed {
        builder = builder.with_fullscreen(Some(Fullscreen::Borderless(None)));
    }
    let window = Arc::new(builder.build(&event_loop)?);
    window.set_cursor_visible(false);
    let mut renderer = pollster::block_on(Renderer::new(
        window.clone(),
        receiver,
        &settings,
        photo_count,
    ))?;
    let mut last_cursor_position: Option<(f64, f64)> = None;
    event_loop.run(move |event, event_loop| {
        let frame_interval = if renderer.scroll_left_held || renderer.scroll_right_held {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(33)
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + frame_interval));
        match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. } => event_loop.exit(),
                WindowEvent::KeyboardInput { event, .. } => match event.logical_key {
                    Key::Named(key @ (NamedKey::ArrowLeft | NamedKey::ArrowRight))
                        if renderer.style == Style::Scroll =>
                    {
                        renderer.set_scroll_arrow(key, event.state == ElementState::Pressed);
                    }
                    _ => event_loop.exit(),
                },
                WindowEvent::Focused(false) => renderer.release_scroll_arrows(),
                WindowEvent::CursorMoved { position, .. } => {
                    if let Some((x, y)) = last_cursor_position {
                        let dx = position.x - x;
                        let dy = position.y - y;
                        if dx * dx + dy * dy >= 9.0 {
                            event_loop.exit();
                        }
                    }
                    last_cursor_position = Some((position.x, position.y));
                }
                WindowEvent::Resized(size) => renderer.resize(size),
                WindowEvent::RedrawRequested => {
                    if !renderer.update() {
                        eprintln!("No readable photos found");
                        event_loop.exit();
                        return;
                    }
                    match renderer.render() {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            renderer.resize(window.inner_size())
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(wgpu::SurfaceError::Timeout) => {}
                    }
                }
                _ => {}
            },
            Event::AboutToWait => window.request_redraw(),
            _ => {}
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn first_scene_fades_in_smoothly() {
        assert_eq!(startup_opacity(Duration::ZERO), 0.0);
        assert!((startup_opacity(STARTUP_FADE / 2) - 0.5).abs() < 0.001);
        assert_eq!(startup_opacity(STARTUP_FADE), 1.0);
        assert_eq!(startup_opacity(STARTUP_FADE * 2), 1.0);
    }

    #[test]
    fn scroll_layout_bag_uses_each_eligible_pattern_before_repeating() {
        let mut bag = LayoutBag::new(4);
        let mut previous = None;
        for _ in 0..20 {
            let cycle: Vec<_> = (0..SCROLL_PATTERN_COUNT).map(|_| bag.next()).collect();
            assert_eq!(
                cycle.iter().copied().collect::<HashSet<_>>().len(),
                SCROLL_PATTERN_COUNT
            );
            if let Some(previous) = previous {
                assert_ne!(cycle[0], previous);
            }
            previous = cycle.last().copied();
        }
        assert_eq!(LayoutBag::new(1).available, vec![0]);
        assert!(!LayoutBag::new(3).available.contains(&6));
    }

    #[test]
    fn every_scroll_pattern_has_valid_card_bounds() {
        let width = 1280.0;
        let height = 800.0;
        let rows = ScrollRows::new(height);
        let near = |a: f32, b: f32| (a - b).abs() < 0.001;
        for pattern in 0..SCROLL_PATTERN_COUNT {
            let (group_width, rects) = scroll_layout(pattern, 0.7, width, height);
            assert_eq!(rects.len(), scroll_pattern_photo_count(pattern));
            let mut lowest_edge = 0.0_f32;
            for [x, y, w, h] in rects {
                assert!(x >= 0.0 && y >= 0.0);
                assert!(x + w <= group_width + 0.001);
                assert!(y + h <= height + 0.001);
                assert!(near(y, rows.top) || near(y, rows.bottom));
                if near(h, rows.full) {
                    assert!(near(y, rows.top));
                } else {
                    assert!(near(h, rows.half));
                }
                lowest_edge = lowest_edge.max(y + h);
            }
            assert!(near(lowest_edge, rows.top + rows.full));
        }
    }

    #[test]
    fn portrait_and_landscape_cards_share_one_pixel_corner_radius() {
        let (screen_w, screen_h) = (1280.0, 800.0);
        let original_portrait_radius = portrait_card_size(screen_w, screen_h).0 * 0.18;
        for pattern in 0..SCROLL_PATTERN_COUNT {
            let (_, rects) = scroll_layout(pattern, 0.7, screen_w, screen_h);
            for [_, _, width, height] in rects {
                let radius = card_corner_radius(screen_w, screen_h, width, height);
                assert!((radius - original_portrait_radius).abs() < 0.001);
            }
        }
        assert_eq!(card_corner_radius(screen_w, screen_h, 100.0, 100.0), 28.0);
    }

    #[test]
    fn polaroid_top_and_side_margins_match_in_pixels() {
        for (width, height) in [(460.0, 688.0), (320.0, 720.0)] {
            let [left, top, right, bottom] = polaroid_photo_bounds(width / height);
            assert!((left * width - top * height).abs() < 0.001);
            assert!(((1.0 - right) * width - top * height).abs() < 0.001);
            assert!(bottom > top);
        }
    }

    #[test]
    fn stamp_has_even_paper_margins_in_both_orientations() {
        for (width, height) in [(460.0, 688.0), (620.0, 328.0)] {
            let radius = card_corner_radius(1280.0, 800.0, width, height);
            let [left, top, right, bottom] = stamp_photo_bounds(width, height, radius);
            let inset = radius * 0.55;
            for margin in [
                left * width,
                top * height,
                (1.0 - right) * width,
                (1.0 - bottom) * height,
            ] {
                assert!((margin - inset).abs() < 0.001);
            }
        }
    }

    #[test]
    fn arch_selects_vertical_scroll_cards() {
        let (_, horizontal_cards) = scroll_layout(4, 1.6, 1280.0, 800.0);
        let (_, portrait_cards) = scroll_layout(3, 0.7, 1280.0, 800.0);
        assert!(portrait_cards.iter().all(is_vertical_card));
        assert!(horizontal_cards.iter().all(|rect| !is_vertical_card(rect)));
        assert_eq!(
            visible_mask(PhotoMask::Arch, &portrait_cards[0]),
            PhotoMask::Arch
        );
        assert_eq!(
            visible_mask(PhotoMask::Arch, &horizontal_cards[0]),
            PhotoMask::None
        );
        assert_eq!(
            visible_mask(PhotoMask::Polaroid, &portrait_cards[0]),
            PhotoMask::Polaroid
        );
        assert_eq!(
            visible_mask(PhotoMask::Polaroid, &horizontal_cards[0]),
            PhotoMask::None
        );
        assert_eq!(
            visible_mask(
                PhotoMask::LargeCorners(TOP_LEFT | BOTTOM_RIGHT),
                &horizontal_cards[0]
            ),
            PhotoMask::LargeCorners(TOP_LEFT | BOTTOM_RIGHT)
        );
        assert_eq!(
            visible_mask(PhotoMask::Stamp(0), &horizontal_cards[0]),
            PhotoMask::Stamp(0)
        );
    }

    #[test]
    fn random_masks_include_each_variant() {
        use rand::SeedableRng;

        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let masks: HashSet<_> = (0..1000).map(|_| random_photo_mask(&mut rng)).collect();
        assert!(masks.contains(&PhotoMask::None));
        assert!(masks.contains(&PhotoMask::Arch));
        assert!(masks.contains(&PhotoMask::Polaroid));
        for color in 0..STAMP_COLOR_COUNT {
            assert!(masks.contains(&PhotoMask::Stamp(color)));
        }
        assert!(masks.contains(&PhotoMask::LargeCorners(TOP_LEFT | BOTTOM_RIGHT)));
        assert!(masks.contains(&PhotoMask::LargeCorners(TOP_RIGHT | BOTTOM_LEFT)));
        assert!(masks.contains(&PhotoMask::LargeCorners(
            TOP_LEFT | TOP_RIGHT | BOTTOM_RIGHT | BOTTOM_LEFT
        )));
        for corner in [TOP_LEFT, TOP_RIGHT, BOTTOM_RIGHT, BOTTOM_LEFT] {
            assert!(masks.contains(&PhotoMask::LargeCorners(corner)));
        }
    }

    #[test]
    fn paper_frames_have_three_other_cards_between_them() {
        use rand::SeedableRng;

        let mut rng = rand::rngs::StdRng::seed_from_u64(17);
        let mut paper_cooldown: usize = 0;
        let mut paper_count = 0;
        for _ in 0..1000 {
            let mask = choose_photo_mask(&mut rng, false, paper_cooldown);
            assert!(!(paper_cooldown > 0 && is_paper_mask(mask)));
            paper_cooldown = if is_paper_mask(mask) {
                3
            } else {
                paper_cooldown.saturating_sub(1)
            };
            paper_count += usize::from(is_paper_mask(mask));
        }
        assert!(paper_count > 0);
    }

    #[test]
    fn quiet_mode_reduces_special_frames() {
        use rand::SeedableRng;

        let mut normal = rand::rngs::StdRng::seed_from_u64(21);
        let mut quiet = rand::rngs::StdRng::seed_from_u64(21);
        let normal_count = (0..1000)
            .filter(|_| choose_photo_mask(&mut normal, false, 0) != PhotoMask::None)
            .count();
        let quiet_count = (0..1000)
            .filter(|_| choose_photo_mask(&mut quiet, true, 0) != PhotoMask::None)
            .count();
        assert!(quiet_count < normal_count);
    }

    #[test]
    fn exif_capture_date_is_preferred_when_available() {
        let mut exif = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        exif.extend_from_slice(&1_u16.to_le_bytes());
        exif.extend_from_slice(&0x8769_u16.to_le_bytes());
        exif.extend_from_slice(&4_u16.to_le_bytes());
        exif.extend_from_slice(&1_u32.to_le_bytes());
        exif.extend_from_slice(&26_u32.to_le_bytes());
        exif.extend_from_slice(&0_u32.to_le_bytes());
        exif.extend_from_slice(&1_u16.to_le_bytes());
        exif.extend_from_slice(&0x9003_u16.to_le_bytes());
        exif.extend_from_slice(&2_u16.to_le_bytes());
        exif.extend_from_slice(&20_u32.to_le_bytes());
        exif.extend_from_slice(&44_u32.to_le_bytes());
        exif.extend_from_slice(&0_u32.to_le_bytes());
        exif.extend_from_slice(b"2026:10:07 12:34:56\0");
        assert_eq!(
            exif_capture_date(exif),
            NaiveDate::from_ymd_opt(2026, 10, 7)
        );
    }

    #[test]
    fn scroll_groups_use_small_gutters_and_preserve_positions() {
        assert_eq!(parse_style("scroll"), Ok(Style::Scroll));
        assert_eq!(parse_style("slides"), Ok(Style::Slides));
        assert!(parse_style("unknown").is_err());
        assert_eq!(default_duration(Style::Scroll), Duration::from_secs(48));
        assert_eq!(default_duration(Style::Slides), Duration::from_secs(8));
        assert_eq!(parse_scroll_speed("1.25"), Ok(Duration::from_secs(48)));
        assert_eq!(parse_scroll_speed("1"), Ok(Duration::from_secs(60)));
        assert!(parse_scroll_speed("0").is_err());
        assert!(parse_scroll_speed("invalid").is_err());
        let (solo_width, solo_rects) = scroll_layout(0, 0.7, 1280.0, 800.0);
        let (pair_width, pair_rects) = scroll_layout(1, 1.0, 1280.0, 800.0);
        let (mosaic_width, mosaic_rects) = scroll_layout(2, 1.0, 1280.0, 800.0);
        let (_, narrow_portrait_rects) = scroll_layout(0, 0.55, 1280.0, 800.0);
        assert_eq!(solo_rects[0], mosaic_rects[0]);
        assert_eq!(solo_rects[0], narrow_portrait_rects[0]);
        assert_eq!(solo_rects[0][1], pair_rects[0][1]);
        assert_eq!(
            solo_rects[0][1] + solo_rects[0][3],
            pair_rects[1][1] + pair_rects[1][3]
        );
        assert_eq!(
            (solo_rects.len(), pair_rects.len(), mosaic_rects.len()),
            (1, 2, 3)
        );
        let groups = [
            ScrollGroup {
                first_photo: 0,
                width: solo_width,
                rects: solo_rects,
            },
            ScrollGroup {
                first_photo: 1,
                width: pair_width,
                rects: pair_rects,
            },
            ScrollGroup {
                first_photo: 3,
                width: mosaic_width,
                rects: mosaic_rects,
            },
        ];
        let gap = 20.0;
        let before = scroll_group_lefts(1280.0, &groups, gap);
        assert!((before[1] + pair_width + gap - before[0]).abs() < 0.001);
        let after = scroll_group_lefts(before[1], &groups[1..], gap);
        assert_eq!(after[0], before[1]);
    }

    #[test]
    fn scroll_preload_reaches_a_complete_group() {
        let mut patterns = VecDeque::from([2, 6, 3, 5, 0]);
        // Ten photos would stop one photo into the fourth layout.
        assert_eq!(scroll_photo_limit(&patterns, 10), 12);
        patterns.pop_front();
        assert_eq!(scroll_photo_limit(&patterns, 10), 10);
    }

    #[test]
    fn held_arrows_move_by_elapsed_time_and_release_pauses_scroll() {
        let left = scroll_motion(0.016, 1280.0, 48.0, true, false, true);
        let right = scroll_motion(0.016, 1280.0, 48.0, false, true, true);
        assert!(left > 0.0);
        assert_eq!(right, -left);
        assert_eq!(
            scroll_motion(0.032, 1280.0, 48.0, false, true, true),
            right * 2.0
        );
        assert_eq!(scroll_motion(0.016, 1280.0, 48.0, false, false, true), 0.0);
        assert_eq!(scroll_motion(0.016, 1280.0, 48.0, true, true, true), 0.0);
        assert!(scroll_motion(0.016, 1280.0, 48.0, false, false, false) > 0.0);
    }

    #[test]
    fn transition_waits_for_the_complete_incoming_layout() {
        assert_eq!(layout_mode(0, 10), 0);
        assert_eq!(layout_mode(1, 10), 1);
        assert_eq!(layout_mode(2, 10), 2);
        assert!(!next_layout_ready(0, 10, 2));
        assert!(next_layout_ready(0, 10, 3));
        assert!(!next_layout_ready(1, 10, 3));
        assert!(next_layout_ready(1, 10, 4));
        assert!(!next_layout_ready(0, 1, 4));
        assert_eq!(layout_rects(2, 1280.0, 800.0).len(), 3);
    }

    #[test]
    fn parses_background_hex_color() {
        assert_eq!(parse_background_color("#F5F0E6"), Ok([245, 240, 230]));
        assert_eq!(parse_background_color("80aBcD"), Ok([128, 171, 205]));
        assert!(parse_background_color("#fff").is_err());
        assert!(parse_background_color("#GG0000").is_err());
    }

    #[test]
    fn clear_color_accounts_for_srgb_surface() {
        let color = background_clear_color([128, 0, 255], true);
        assert!((color.r - 0.21586).abs() < 0.0001);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 1.0);
    }

    #[test]
    fn applies_jpeg_exif_orientation() {
        let image = image::RgbImage::from_pixel(2, 1, image::Rgb([255, 0, 0]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image)
            .unwrap();
        // APP1 Exif segment with orientation 8 (rotate 270 degrees clockwise).
        let exif = [
            0xff, 0xe1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1,
            0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0,
        ];
        let mut oriented_jpeg = jpeg[..2].to_vec();
        oriented_jpeg.extend_from_slice(&exif);
        oriented_jpeg.extend_from_slice(&jpeg[2..]);
        let path = std::env::temp_dir().join(format!(
            "ambient-orientation-{}-{}.jpg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, oriented_jpeg).unwrap();
        let result = open_oriented(&path);
        std::fs::remove_file(&path).unwrap();
        let (image, date) = result.unwrap();
        assert_eq!((image.width(), image.height()), (1, 2));
        assert!(date.is_some());
    }
}
