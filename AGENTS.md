# Repository notes for agents

- This is a Rust 2021 application for an Ubuntu ambient photo display. Read `README.md` for the product goal, current behavior, and roadmap.
- `src/main.rs` owns CLI parsing, asynchronous image loading, the `wgpu` renderer, layouts, and the `winit` event loop. `src/photos.rs` handles recursive discovery and shuffle order. `src/shader.wgsl` is the photo shader.
- Apply image orientation metadata before resizing or uploading photos. Many camera JPEGs store landscape pixels with an EXIF rotation tag.
- A slide transition needs every photo in the incoming layout preloaded. Draw that complete layout during the fade, and start the fade timer only when it is ready, so layouts do not snap into place afterward.
- The default `scroll` style uses variable-width photo groups in one continuous rightward strip, separated by small gutters. The default speed is one screen width per 32 seconds; `--style slides` keeps the older eight-second fade and slide timing. Keep enough groups loaded to cover the incoming edge, and preserve exact group positions when the rightmost group leaves the screen.
- Full-height portrait slots in solo and mosaic scroll groups share `portrait_card_size`; keep their dimensions and vertical edges aligned when changing layouts.
- Keep photo sources, layouts, rendering, and future idle integration conceptually separate. Prefer Wayland-compatible desktop APIs; avoid making X11 or XScreenSaver a required dependency.
- Wayland sends an initial `CursorMoved` event when a fullscreen window opens. The first position is a baseline; do not make the app exit on that event. Real keyboard, mouse, and pointer activity should close the display.
- Run `cargo fmt --check`, `cargo test`, and `cargo clippy -- -D warnings` after code changes. For rendering or input changes, also launch a short windowed or fullscreen smoke test with photos. The `photos/` directory is local test data and is gitignored.
