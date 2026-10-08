# Publish a release

The first archive is built against Ubuntu 24.04 libraries for x86-64. It has been tested on Ubuntu 26.04.1; test the full GNOME flow on any other Ubuntu version before claiming support for it.

## Prepare the files

1. Set the version in `Cargo.toml` and update the archive name in `README.md` if it changed. Commit the release files and push the commit to GitHub.
2. On an x86-64 machine with Docker and Rust installed through rustup, run `scripts/package-release.sh` from the repo root. The script builds in an Ubuntu 24.04 container and writes two ignored files under `dist/`:
   - `ambient-screensaver-v0.1.0-ubuntu-24.04-x86_64.tar.gz`
   - `ambient-screensaver-v0.1.0-ubuntu-24.04-x86_64.tar.gz.sha256`
3. Verify the download before uploading:

   ```sh
   cd dist
   sha256sum -c ambient-screensaver-v0.1.0-ubuntu-24.04-x86_64.tar.gz.sha256
   tar -tzf ambient-screensaver-v0.1.0-ubuntu-24.04-x86_64.tar.gz
   ```

4. Extract the archive on the Ubuntu version being released. Run `./ambient-screensaver gnome check`, follow the README's `gnome install` command, and open the display with `--windowed` to check graphics and fonts.

## Create the GitHub release

1. Open the repository on GitHub and select **Releases → Draft a new release**.
2. In **Choose a tag**, create `v0.1.0` and target the pushed release commit on `main`. Keep the tag version aligned with `Cargo.toml`.
3. Use `v0.1.0` as the title. A suitable description is: “First preview of Ambient Photos for Ubuntu GNOME. Includes scrolling photo layouts, a clock, and per-user GNOME idle setup. Download the Ubuntu x86-64 archive below and follow the README. Tested on Ubuntu 26.04.1.” Mark it as a pre-release if you want to signal that this is still a preview.
4. Upload **both** files from `dist/` as release assets. GitHub's automatic “Source code (tar.gz)” is a source snapshot; users need the uploaded `ambient-screensaver-...tar.gz` file.
5. Review the tag, title, notes, and both assets, then publish.

Do not commit `dist/` to Git. The tag must point to the same source version used for the uploaded binary. If code changes after the build, rebuild and retest the archive before publishing.
