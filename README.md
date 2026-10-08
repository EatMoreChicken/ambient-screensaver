# Ambient Photos

A standalone ambient photo display for Ubuntu, inspired by ChromeOS and Google Ambient Mode. It is built in Rust with `wgpu` and `winit`, with Wayland compatibility as a priority. You can launch it directly or install its GNOME idle integration for your user account.

## Quick start and removal (planned release)

**The downloadable archive is not published yet.** This is the intended setup for Ubuntu Desktop users once it is available. For now, use the [source-build instructions](#set-up-gnome-idle-activation) below.

Download the Linux x86-64 `.tar.gz` archive from the project's future Releases page, then run these commands in the directory where you downloaded it:

```sh
tar -xzf ambient-screensaver-linux-x86_64.tar.gz
cd ambient-screensaver-linux-x86_64
./ambient-screensaver gnome install --idle-seconds 120 "$HOME/Pictures"
```

The archive will contain the compiled `ambient-screensaver` executable. Replace `"$HOME/Pictures"` with a directory that contains your photos; you can add more directories to the same command. The installer copies the executable into your user account, saves the photo paths, and starts it automatically at your next GNOME login. You can delete the extracted archive directory after installation. Rust and Cargo are not needed to use the downloaded build.

To update, download and extract the newer archive, then run its `gnome install` command again with your photo directories and idle timeout. There is no need to uninstall first. The command replaces the installed binary and settings; a watcher already running keeps using the previous version until your next GNOME login. You can check your current paths and timeout with `gnome status` before updating.

To check the installed setup or remove it later:

```sh
~/.local/share/ambient-screensaver/bin/ambient-screensaver gnome status
~/.local/share/ambient-screensaver/bin/ambient-screensaver gnome uninstall
```

Removal deletes the installed executable, saved settings, and login entry. If the idle watcher is already running, it stops when you log out. Keep your photo directories in place for as long as you use the app.

## Current state

- Reads photos recursively from one or more directories, applies image orientation metadata, shuffles without repeating within a cycle, and preloads on a worker thread. Supported formats are JPEG, PNG, WebP, GIF, BMP, and TIFF. Videos are not supported.
- Defaults to a continuous, rightward scroll with small gutters and eight shuffled layout patterns. These include single photos, portrait pairs, stacked photos, and mosaics. The photo rows and outer edges stay aligned across groups.
- Offers `--style slides` for the earlier fading and sliding presentation with one, two, or three photos per layout. It waits for the full incoming layout before starting a transition.
- Fades in the first complete photo scene over 1.5 seconds. A small, centered line in the bottom mat shows the local weekday, date, and time; it updates each minute and remains visible while photos load.
- Starts fullscreen, hides the pointer, and closes on keyboard, mouse, or pointer activity. `--windowed` opens a resizable preview.

This is still a development preview. GNOME setup works from a source build; the downloadable archive described above is future work.

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

## Set up GNOME idle activation

The Rust application talks to GNOME's idle monitor directly. It starts the photo display after the configured inactivity period, skips launch while the screen is locked, and waits for user activity before another launch. It does not replace GNOME's lock screen. Set the idle threshold below GNOME's blank-screen or lock timeout.

From the repo root, build and check the GNOME connection, then install for your user account:

```sh
cargo build --release
./target/release/ambient-screensaver gnome check
./target/release/ambient-screensaver gnome install --idle-seconds 120 /path/to/photos
```

`install` copies the compiled app to `~/.local/share/ambient-screensaver/bin/`, saves the absolute photo paths and timeout in `~/.config/ambient-screensaver/gnome.json`, and adds a GNOME-only login entry under `~/.config/autostart/`. It needs no root access or Python process. The source checkout can move after installation if the photo directories remain at their saved paths. The default threshold is 120 seconds. Run `install` again to update the settings or installed binary after rebuilding. If you use custom `XDG_CONFIG_HOME` or `XDG_DATA_HOME` directories, those replace the default locations.

To check the setup or test it immediately in the current session, use the installed copy in a terminal:

```sh
~/.local/share/ambient-screensaver/bin/ambient-screensaver gnome status
~/.local/share/ambient-screensaver/bin/ambient-screensaver gnome run
```

For a quick test before the next login, install with `--idle-seconds 15`, run `gnome run`, wait 15 seconds without input, then move the mouse to close the display and press Ctrl+C to stop the watcher. Re-run `install --idle-seconds 120 /path/to/photos` for normal use. At the next GNOME login, the watcher starts automatically. If it is already running, `gnome run` reports that another watcher is active; use `gnome status` to check the installation instead.

To remove the GNOME setup, run:

```sh
~/.local/share/ambient-screensaver/bin/ambient-screensaver gnome uninstall
```

Uninstall removes the copied binary, saved settings, and login entry. A watcher already running in this session exits when you log out or stop it with Ctrl+C.

## Roadmap

- [x] Add native GNOME login startup with saved photo paths and an idle timeout.
- [ ] Add saved display options to the GNOME setup command.
- [ ] Continue refining the visuals and consider gentle pan and zoom, rounded photo cards, soft shadows, and alternate backgrounds.
- [ ] Explore photo metadata, face-aware cropping, weather, and multi-monitor behavior.
- [ ] More fun shapes and layouts

### Packaging and release

- [ ] Publish a tested Linux x86-64 `.tar.gz` containing the compiled binary and brief instructions, following the quick start above.
- [ ] Add automated build and test workflows for tagged releases; consider more CPU architectures and an Ubuntu `.deb` afterward.

Replacing the GNOME lock screen or handling authentication is outside the planned scope.

## Test

```sh
cargo fmt --check
cargo test
cargo clippy -- -D warnings
```
