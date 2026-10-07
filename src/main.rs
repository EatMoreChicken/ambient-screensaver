mod photos;

use image::{metadata::Orientation, DynamicImage, ImageDecoder, ImageReader};
use std::{
    collections::VecDeque,
    env,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use winit::{
    dpi::PhysicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Fullscreen, Window, WindowBuilder},
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
}

struct Settings {
    directories: Vec<PathBuf>,
    duration: Duration,
    transition: Duration,
    background_color: [u8; 3],
    windowed: bool,
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
    let mut settings = Settings {
        directories: Vec::new(),
        duration: Duration::from_secs(8),
        transition: Duration::from_millis(1500),
        background_color: [0xF5, 0xF0, 0xE6],
        windowed: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Usage: ambient-screensaver [--windowed] [--duration SECONDS] [--transition SECONDS] [--background-color '#RRGGBB'] PHOTO_DIR [PHOTO_DIR ...]");
                std::process::exit(0);
            }
            "--windowed" => settings.windowed = true,
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
    Ok(settings)
}

struct DecodedPhoto {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

fn open_oriented(path: &Path) -> Result<DynamicImage, Box<dyn std::error::Error + Send + Sync>> {
    let mut decoder = ImageReader::open(path)?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn start_loader(paths: Vec<PathBuf>) -> mpsc::Receiver<DecodedPhoto> {
    let (sender, receiver) = mpsc::sync_channel(3);
    std::thread::spawn(move || {
        let count = paths.len();
        let mut bag = photos::ShuffleBag::new(paths);
        let mut failures = 0;
        while let Some(path) = bag.next() {
            match open_oriented(&path) {
                Ok(image) => {
                    failures = 0;
                    let image = if image.width() > 2560 || image.height() > 2560 {
                        image.resize(2560, 2560, image::imageops::FilterType::Lanczos3)
                    } else {
                        image
                    }
                    .to_rgba8();
                    if sender
                        .send(DecodedPhoto {
                            width: image.width(),
                            height: image.height(),
                            pixels: image.into_raw(),
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
    bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
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
    photos: VecDeque<Photo>,
    receiver: mpsc::Receiver<DecodedPhoto>,
    slide_started: Instant,
    duration: Duration,
    transition: Duration,
    slide_number: u64,
}

impl Renderer {
    async fn new(
        window: Arc<Window>,
        receiver: mpsc::Receiver<DecodedPhoto>,
        settings: &Settings,
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
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32],
            }] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_main", targets: &[Some(wgpu::ColorTargetState {
                format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL,
            })] }),
            primitive: wgpu::PrimitiveState::default(), depth_stencil: None,
            multisample: wgpu::MultisampleState::default(), multiview: None,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("card vertices"),
            size: (6 * 4 * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            surface,
            device,
            queue,
            config,
            background_color,
            pipeline,
            bind_layout,
            sampler,
            vertices,
            photos: VecDeque::new(),
            receiver,
            slide_started: Instant::now(),
            duration: settings.duration,
            transition: settings.transition,
            slide_number: 0,
        })
    }

    fn upload(&self, decoded: DecodedPhoto) -> Photo {
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
            bind_group,
            _texture: texture,
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    fn update(&mut self) -> bool {
        while self.photos.len() < 3 {
            match self.receiver.try_recv() {
                Ok(decoded) => self.photos.push_back(self.upload(decoded)),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return !self.photos.is_empty(),
            }
        }
        if self.photos.len() > 1 && self.slide_started.elapsed() >= self.duration + self.transition
        {
            self.photos.pop_front();
            self.slide_started = Instant::now();
            self.slide_number += 1;
        }
        true
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ambient frame"),
            });
        let mut cards: Vec<(&Photo, [f32; 4], f32)> = Vec::new();
        let elapsed = self.slide_started.elapsed();
        let progress = if self.photos.len() > 1 {
            ((elapsed.as_secs_f32() - self.duration.as_secs_f32()) / self.transition.as_secs_f32())
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let width = self.config.width as f32;
        let height = self.config.height as f32;
        let mode = self.slide_number % 3;
        if let Some(photo) = self.photos.front() {
            let rect = if mode == 1 && self.photos.len() >= 2 {
                [
                    width * 0.045 - eased * width * 0.08,
                    height * 0.08,
                    width * 0.47,
                    height * 0.84,
                ]
            } else if mode == 2 && self.photos.len() >= 3 {
                [
                    width * 0.04 - eased * width * 0.08,
                    height * 0.06,
                    width * 0.57,
                    height * 0.88,
                ]
            } else {
                let mut rect = fitted_rect(
                    photo,
                    [
                        width * 0.035 - eased * width * 0.08,
                        height * 0.04,
                        width * 0.93,
                        height * 0.92,
                    ],
                );
                let drift = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);
                rect[0] += width * 0.012 * drift;
                rect[1] -= height * 0.008 * drift;
                rect
            };
            cards.push((photo, rect, 1.0 - eased));
        }
        if let Some(photo) = self.photos.get(1) {
            if mode == 1 {
                cards.push((
                    photo,
                    [width * 0.535, height * 0.08, width * 0.42, height * 0.84],
                    1.0 - eased,
                ));
            } else if mode == 2 && self.photos.len() >= 3 {
                cards.push((
                    photo,
                    [width * 0.64, height * 0.06, width * 0.32, height * 0.42],
                    1.0 - eased,
                ));
            }
        }
        if mode == 2 {
            if let Some(photo) = self.photos.get(2) {
                cards.push((
                    photo,
                    [width * 0.64, height * 0.52, width * 0.32, height * 0.42],
                    1.0 - eased,
                ));
            }
        }
        if progress > 0.0 {
            if let Some(photo) = self.photos.get(1) {
                let rect = fitted_rect(
                    photo,
                    [
                        width * (0.115 - eased * 0.08),
                        height * 0.04,
                        width * 0.93,
                        height * 0.92,
                    ],
                );
                cards.push((photo, rect, eased));
            }
        }
        let stride = (6 * std::mem::size_of::<Vertex>()) as u64;
        for (index, (photo, rect, alpha)) in cards.iter().enumerate() {
            let vertices = card_vertices(rect, *alpha, photo, width, height);
            self.queue.write_buffer(
                &self.vertices,
                index as u64 * stride,
                bytemuck::cast_slice(&vertices),
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
            for (index, (photo, _, _)) in cards.iter().enumerate() {
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
    let (u0, u1, v0, v1) = if image_aspect > box_aspect {
        let span = box_aspect / image_aspect;
        ((1.0 - span) / 2.0, (1.0 + span) / 2.0, 0.0, 1.0)
    } else {
        let span = image_aspect / box_aspect;
        (0.0, 1.0, (1.0 - span) / 2.0, (1.0 + span) / 2.0)
    };
    let vertex = |px, py, u, v| Vertex {
        position: [px, py],
        uv: [u, v],
        alpha,
    };
    [
        vertex(x0, y0, u0, v0),
        vertex(x0, y1, u0, v1),
        vertex(x1, y1, u1, v1),
        vertex(x0, y0, u0, v0),
        vertex(x1, y1, u1, v1),
        vertex(x1, y0, u1, v0),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings()?;
    let paths = photos::discover(&settings.directories);
    if paths.is_empty() {
        return Err("no supported photos found in the supplied directories".into());
    }
    eprintln!("Found {} photos", paths.len());
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
    let mut renderer = pollster::block_on(Renderer::new(window.clone(), receiver, &settings))?;
    let mut last_cursor_position: Option<(f64, f64)> = None;
    event_loop.run(move |event, event_loop| {
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(33),
        ));
        match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                WindowEvent::CloseRequested
                | WindowEvent::KeyboardInput { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. } => event_loop.exit(),
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
        let image = result.unwrap();
        assert_eq!((image.width(), image.height()), (1, 2));
    }
}
