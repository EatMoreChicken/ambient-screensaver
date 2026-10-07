# Ambient Photos

A standalone ambient photo display for Ubuntu, inspired by ChromeOS and Google Ambient Mode. The goal is a calm, varied presentation of personal photos that appears when the desktop is idle and disappears when the user returns. It is built in Rust with `wgpu` and `winit`, with Wayland compatibility as a priority.

The current version is the visual first milestone: it runs when launched, reads images recursively from local directories, shuffles without repeating within a cycle, preloads images on a worker thread, and renders solo and multi-photo layouts with gentle movement, fades, and sliding transitions. Portrait and landscape images are fitted or cropped according to the layout.

## Build and run

Install a Rust toolchain, then run:

```sh
cargo run --release -- /path/to/photos
```

You can provide multiple directories. The app supports JPEG, PNG, WebP, GIF, BMP, and TIFF images. It starts fullscreen and hides the pointer. Press a key, click, scroll, or move the mouse to leave the display.

For a resizable preview:

```sh
cargo run --release -- --windowed --duration 8 --transition 1.5 /path/to/photos
```

`--duration` controls how long a layout stays on screen before its transition begins; `--transition` controls the transition length. Both values are in seconds.

## Direction and roadmap

Keep photo discovery, layout decisions, GPU rendering, and idle integration separate. The visual experience should stay independent of any particular screensaver or desktop mechanism. Prefer Wayland-compatible Ubuntu/GNOME integration; XScreenSaver could be added later as an optional launcher, but is not the core architecture.

The next milestone is configurable idle activation and stopping the display when the user returns. A simple config file could then cover photo directories, idle timeout, photo duration, transition speed, layout choices, shuffle behavior, background style, and multi-monitor behavior.

Visual improvements to explore include more varied mosaics, gentle pan and zoom, rounded cards and soft shadows, and blurred or photo-colored backgrounds. Longer-term possibilities include photo metadata, face-aware cropping, coordinated multi-monitor layouts, clock or weather overlays, and sources such as Google Photos exports, Syncthing folders, Immich, or Nextcloud.

Replacing the GNOME lock screen or handling authentication is outside the initial scope.

## Test

```sh
cargo test
```
