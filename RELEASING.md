# Releasing Ambient Photos

Use this checklist for each version. The package script currently builds an x86-64 binary against Ubuntu 24.04 libraries. Test the full GNOME flow on each Ubuntu version you claim to support.

## Build and check the archive

1. Update the version in `Cargo.toml` and the archive name in `README.md`. Commit the release changes and push that commit to GitHub.
2. On an x86-64 machine with Docker and Rust installed through rustup, run `scripts/package-release.sh` from the repo root. It prints the paths to an archive and its `.sha256` file under ignored `dist/`.
3. Verify the checksum and archive contents, including both font license notices, replacing `VERSION` with the version in `Cargo.toml`:

   ```sh
   cd dist
   sha256sum -c ambient-screensaver-vVERSION-ubuntu-24.04-x86_64.tar.gz.sha256
   tar -tzf ambient-screensaver-vVERSION-ubuntu-24.04-x86_64.tar.gz
   ```

4. Extract the archive on a target Ubuntu desktop. Run `./ambient-screensaver gnome check`, install it with a photo folder as shown in `README.md`, and open it with `--windowed` to check graphics and fonts. If anything changes after this test, rebuild and retest.

## Create the GitHub release

1. Open the repository on GitHub and select **Releases → Draft a new release**.
2. In **Choose a tag**, create `vVERSION` targeting the pushed release commit. The tag must match `Cargo.toml` and the archive name.
3. Use `Ambient Photos vVERSION` as the title. In the description, summarize the changes, give the Ubuntu versions actually tested, mention any limitations, and point readers to the README installation steps. Mark it as a pre-release if it is still a preview.
4. Upload both files printed by the package script as release assets: the `.tar.gz` and its `.sha256`. GitHub's automatic “Source code (tar.gz)” is a source snapshot, not the binary archive.
5. Review the tag, title, notes, and assets, then publish.

Do not commit `dist/` to Git. The tag must point to the source used for the uploaded binary.
