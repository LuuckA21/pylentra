# Pylentra

Pylentra is a desktop SSH connection manager for Linux and Windows. Keep your
servers in a searchable library with groups, open them in terminal tabs, chain
jump hosts, and manage local and remote port forwards, with secrets stored in
the OS keychain and signed automatic updates.

- Quick connect: type `user@host` and go, then save it if you like.
- Import your servers from `~/.ssh/config` with a preview.
- Colors and icons for connections and groups; a welcome screen with recent
  connections.
- Terminal themes (Dracula, Nord, Solarized), automatic reconnection and
  confirmation before closing live sessions.

Built with Rust, [Tauri 2](https://tauri.app) and Svelte. The UI is available
in English and Italian.

## Install

Download the latest build from the
[Releases page](https://github.com/LuuckA21/pylentra/releases/latest) and
check it against `SHA256SUMS.txt` (`sha256sum -c SHA256SUMS.txt`).

- **Linux, AppImage:** `chmod +x Pylentra_*.AppImage && ./Pylentra_*.AppImage`
- **Linux, Debian/Ubuntu:** `sudo apt install ./Pylentra_*_amd64.deb`
- **Windows:** run the `Pylentra_*-setup.exe` installer (per-user, no admin
  rights needed).

Once installed, Pylentra checks for updates at startup and shows a banner
offering to install and restart. On `.deb` installs, installing the update
asks for your administrator password (it runs `dpkg -i` through `pkexec`,
`zenity` or `kdialog`); you can also install the new `.deb` by hand.

## Known limitations

- **RSA user keys.** The underlying `rsa` crate has a known timing
  side-channel ([RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071),
  no upstream fix). It is only exploitable by a hostile or compromised server
  that times your signatures during authentication. Prefer ed25519 keys.
- **Linux without a Secret Service** (GNOME Keyring, KWallet, ...): saved
  credentials cannot be stored, so they are asked at every connection. If you
  start a keyring after Pylentra, restart the app.
- **Windows builds are unsigned.** SmartScreen shows "Windows protected your
  PC": click **More info**, then **Run anyway**.
- **`known_hosts`:** `@revoked` and `@cert-authority` markers and wildcard host
  patterns in `~/.ssh/known_hosts` are not honoured. Pylentra only reads that
  file and never writes to `~/.ssh`.
- **macOS is not supported** in v1.

## Build from source

Requirements: Rust 1.89 or newer, Node 22, pnpm 10, and on Linux the Tauri
system packages:

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
```

```sh
pnpm install
pnpm tauri dev     # run in development
pnpm tauri build   # AppImage + .deb on Linux, NSIS installer on Windows
```

`pnpm tauri build` produces signed updater artifacts, so it needs the signing
key (see below). For a local build without it:

```sh
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

Checks: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `pnpm --dir ui lint|check|test`.

### Icons

The sources are `src-tauri/icons/source/icon.svg` (full size) and `icon-small.svg`
(hand-tuned for 16 to 32 px). After editing them, from the repo root run, in this order:

```sh
pnpm tauri icon src-tauri/icons/source/icon.svg   # regenerates src-tauri/icons/
python3 scripts/icons-small.py                    # small-size PNG and icon.ico (pip install cairosvg pillow)
```

`tauri icon` also writes Square*Logo.png, StoreLogo.png, android/, ios/ and 64x64.png,
which the app does not use; delete them before committing.

## Releasing

One-time setup of the updater signing key:

1. `pnpm tauri signer generate -w ~/.tauri/pylentra.key`. Keep the private key
   and its password safe; never commit them. Losing the key means installed
   copies can no longer auto-update.
2. Paste the printed public key into `plugins.updater.pubkey` in
   `src-tauri/tauri.conf.json`.
3. Store the key file content as `TAURI_SIGNING_PRIVATE_KEY` and its password
   as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Best as secrets of an environment
   named `release` (Settings → Environments → New environment `release` →
   Deployment branches: *Selected branches*, add `main` → add the two
   secrets), so workflows on other branches cannot read them; then delete
   any repository-level copies. Plain repository secrets also work.

Without Node: the key is a plain [minisign](https://github.com/jedisct1/minisign)
key pair, base64-encoded. Run `minisign -G -p pylentra.pub -s pylentra.key`,
then use the base64 of `pylentra.pub` as `pubkey` and the base64 of
`pylentra.key` as `TAURI_SIGNING_PRIVATE_KEY` (PowerShell:
`[Convert]::ToBase64String([IO.File]::ReadAllBytes("pylentra.key"))`;
Linux: `base64 -w0 pylentra.key`).

The current public key is minisign key ID `590862C6663206ED`.

Per release, open a PR that:

1. bumps `version` in `[workspace.package]` of the root `Cargo.toml` and runs
   `cargo update -w` so `Cargo.lock` follows;
2. adds the `## [X.Y.Z] - YYYY-MM-DD` section to `CHANGELOG.md`: it becomes
   the release notes, and the release stops if it is missing.

Walk through [`docs/release-checklist.md`](docs/release-checklist.md) on that
PR's Preview build bundles. When the PR lands on `main`, the `Release` workflow
sees a version without a tag and:

- checks that the signing key works and matches the public key in the app;
- creates the `vX.Y.Z` tag on that commit and a **draft** release with the
  notes;
- builds and signs Linux (AppImage, `.deb`) and Windows (NSIS) packages;
- verifies every signature, then attaches `latest.json` and `SHA256SUMS.txt`.

Check the draft and publish it. Installed apps only see published releases.
The workflow can also be run by hand on `main` (Actions → Release → Run
workflow) for a version without a tag. If a run fails, use **Re-run failed
jobs**: a fresh run sees the tag the first one created and stops.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
