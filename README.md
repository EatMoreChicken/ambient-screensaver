# Ambient Photos

A standalone ambient photo display for Ubuntu, inspired by ChromeOS and Google Ambient Mode. It is built in Rust with `wgpu` and `winit`, with Wayland compatibility as a priority. You can launch it directly or use the development idle launcher on Ubuntu GNOME.

## Current state

- Reads photos recursively from one or more directories, applies image orientation metadata, shuffles without repeating within a cycle, and preloads on a worker thread. Supported formats are JPEG, PNG, WebP, GIF, BMP, and TIFF. Videos are not supported.
- Defaults to a continuous, rightward scroll with small gutters and eight shuffled layout patterns. These include single photos, portrait pairs, stacked photos, and mosaics. The photo rows and outer edges stay aligned across groups.
- Offers `--style slides` for the earlier fading and sliding presentation with one, two, or three photos per layout. It waits for the full incoming layout before starting a transition.
- Fades in the first complete photo scene over 1.5 seconds. A small, centered line in the bottom mat shows the local weekday, date, and time; it updates each minute and remains visible while photos load.
- Starts fullscreen, hides the pointer, and closes on keyboard, mouse, or pointer activity. `--windowed` opens a resizable preview.

This is still a development preview. A GNOME idle launcher is available, but it must be started manually; saved settings, a desktop launcher, and an installer have not been added yet.

## Build and run

Install a Rust toolchain and run from this repository:

```sh
cargo run --release -- /path/to/photos
```

You can provide multiple directories. Press a key, click, scroll, or move the mouse to leave the display.

For a resizable preview with a custom scroll speed:

```sh
cargo run --release -- --windowed --scroll-speed 1 /path/to/photos
```

The default scroll speed is **1.25 screen widths per minute** (one screen width every 48 seconds). `--scroll-speed` sets screen widths per minute: smaller numbers scroll more slowly, and larger numbers scroll faster. For example, `--scroll-speed 1` takes one minute per screen width. `--duration 60` gives that same speed in scroll mode; use one speed option at a time. The strip pauses if the next group has not loaded.

To use the original fading and sliding presentation:

```sh
cargo run --release -- --style slides --duration 8 --transition 1.5 /path/to/photos
```

With `--style slides`, `--duration` is the time a layout stays on screen before its transition, and `--transition` is the transition length. Both values are in seconds; the defaults are 8 and 1.5 respectively.

The mat behind and between photos defaults to cream white (`#F5F0E6`). Set a different color with a six-digit hex code:

```sh
cargo run --release -- --background-color '#DCE8E0' /path/to/photos
```

The clock text is dark grey and uses the system's DejaVu Sans or Liberation Sans font.

## Start when idle on Ubuntu GNOME

The development launcher uses GNOME's idle monitor to wait for inactivity, then starts the existing screensaver binary. It does not start while the screen is locked. After the display closes, it waits for new user activity before it can launch again. This launcher is specific to Ubuntu/GNOME; it does not replace the lock screen.

From the repo root, build the binary once, check that the GNOME services are available, and run the launcher in a terminal:

```sh
cargo build --release
python3 scripts/idle_launcher.py --check
python3 scripts/idle_launcher.py --idle-seconds 120 /path/to/photos
```

The default idle threshold is two minutes. Set it lower than GNOME's screen lock timeout so the photos can appear before the lock screen. The launcher needs Python 3 and `gdbus` (provided by Ubuntu's `libglib2.0-bin`). Leave this terminal open, or run it as a temporary user service for the current login session:

```sh
systemd-run --user --unit=ambient-photos-idle --collect \
  /usr/bin/python3 "$PWD/scripts/idle_launcher.py" --idle-seconds 120 /path/to/photos
```

Stop that service with `systemctl --user stop ambient-photos-idle.service`. The launcher runs `target/release/ambient-screensaver`; rebuild with `cargo build --release` after code changes. A permanent setup that starts automatically at login is still on the roadmap.

## Roadmap

- [ ] Add a saved idle timeout and automatic startup with the Ubuntu/GNOME graphical session.
- [ ] Add saved settings for photo directories and display options, so launching no longer requires command-line arguments.
- [ ] Continue refining the visuals and consider gentle pan and zoom, rounded photo cards, soft shadows, and alternate backgrounds.
- [ ] Explore photo metadata, face-aware cropping, weather, and multi-monitor behavior.

### Packaging and release

- [ ] Add a first-run way to select photo directories and a desktop launcher with an icon.
- [ ] Package an installable Ubuntu `.deb` so users do not need Rust or Cargo.
- [ ] Add automated build and test workflows, then publish tagged release packages with installation instructions.

Replacing the GNOME lock screen or handling authentication is outside the planned scope.

## Test

```sh
cargo fmt --check
cargo test
cargo clippy -- -D warnings
python3 -m unittest discover -s tests
```
