#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_dir"

if [[ $(uname -m) != x86_64 ]]; then
  echo "This package script currently supports x86-64 builds only." >&2
  exit 1
fi

if ! command -v docker >/dev/null; then
  echo "Docker is required to build against Ubuntu 24.04." >&2
  exit 1
fi

if [[ ! -x $HOME/.cargo/bin/cargo || ! -d $HOME/.rustup ]]; then
  echo "A Rust toolchain installed with rustup is required to build the archive." >&2
  exit 1
fi

version=$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)
if [[ -z $version ]]; then
  echo "Could not read the package version from Cargo.toml." >&2
  exit 1
fi

name="ambient-screensaver-v${version}-ubuntu-24.04-x86_64"
mkdir -p dist
docker build -q -t ambient-screensaver-build:ubuntu-24.04 \
  -f packaging/ubuntu-24.04.Dockerfile packaging >/dev/null
docker run --rm --user "$(id -u):$(id -g)" \
  -v "$repo_dir:$repo_dir" -w "$repo_dir" \
  -v "$HOME/.cargo:$HOME/.cargo" \
  -v "$HOME/.rustup:$HOME/.rustup:ro" \
  -e HOME="$HOME" -e PATH="$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
  -e CARGO_TARGET_DIR="$repo_dir/target/ubuntu-24.04" \
  -e RUSTUP_TOOLCHAIN=stable \
  ambient-screensaver-build:ubuntu-24.04 cargo build --release --locked
tar -czf "dist/$name.tar.gz" \
  --transform="s|^|$name/|" \
  -C target/ubuntu-24.04/release ambient-screensaver \
  -C "$repo_dir" README.md assets/ambient-photos-demo.gif \
  assets/fonts/README.md assets/fonts/fredoka/OFL.txt assets/fonts/caveat/OFL.txt
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")

echo "Created dist/$name.tar.gz"
echo "Created dist/$name.tar.gz.sha256"
