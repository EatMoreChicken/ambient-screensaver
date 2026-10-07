# Ambient Photos

A standalone ambient photo display for Ubuntu, inspired by ChromeOS and Google Ambient Mode. The goal is a calm, varied presentation of personal photos that appears when the desktop is idle and disappears when the user returns. It is built in Rust with `wgpu` and `winit`, with Wayland compatibility as a priority.

The current version is the visual first milestone: it runs when launched, reads images recursively from local directories, shuffles without repeating within a cycle, and preloads images on a worker thread. The default presentation is a continuous rightward strip with small gaps between photo groups. It varies solo, stacked, paired portrait, mosaic, three-photo row, and four-photo grid layouts in shuffled order. Every group shares the same top and bottom edges; full-height cards match each other, while stacked cards use two aligned rows. Portrait and landscape images are fitted or cropped according to the layout.

## Build and run

Install a Rust toolchain, then run:

```sh
cargo run --release -- /path/to/photos
```

You can provide multiple directories. The app supports JPEG, PNG, WebP, GIF, BMP, and TIFF images and applies EXIF orientation when present. It starts fullscreen and hides the pointer. Press a key, click, scroll, or move the mouse to leave the display.

For a resizable preview with a custom scroll speed:

```sh
cargo run --release -- --windowed --scroll-speed 1 /path/to/photos
```

The default scroll speed is **1.25 screen widths per minute** (one screen width every 48 seconds). `--scroll-speed` sets screen widths per minute: smaller numbers scroll more slowly, and larger numbers scroll faster. For example, `--scroll-speed 1` takes one minute per screen width. The existing `--duration 60` option gives that same speed; use one speed option at a time. The strip pauses only if enough photos have not yet loaded to fill the incoming edge.

To use the original fading and sliding presentation:

```sh
cargo run --release -- --style slides --duration 8 --transition 1.5 /path/to/photos
```

With `--style slides`, `--duration` is the time a layout stays on screen before its transition, and `--transition` is the transition length. Both values are in seconds.

The mat behind and between photos defaults to cream white (`#F5F0E6`). Set a different color with a six-digit hex code:

```sh
cargo run --release -- --background-color '#DCE8E0' /path/to/photos
```

## Direction and roadmap

Keep photo discovery, layout decisions, GPU rendering, and idle integration separate. The visual experience should stay independent of any particular screensaver or desktop mechanism. Prefer Wayland-compatible Ubuntu/GNOME integration; XScreenSaver could be added later as an optional launcher, but is not the core architecture.

The next milestone is configurable idle activation and stopping the display when the user returns. A simple config file could then cover photo directories, idle timeout, photo duration, transition speed, layout choices, shuffle behavior, background style, and multi-monitor behavior.

Visual improvements to explore include more varied mosaics, gentle pan and zoom, rounded cards and soft shadows, and blurred or photo-colored backgrounds. Longer-term possibilities include photo metadata, face-aware cropping, coordinated multi-monitor layouts, clock or weather overlays, and sources such as Google Photos exports, Syncthing folders, Immich, or Nextcloud.

Replacing the GNOME lock screen or handling authentication is outside the initial scope.

## Test

```sh
cargo test
```
