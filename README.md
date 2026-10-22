# Pylentra

Pylentra is a desktop SSH connection manager for Linux and Windows. Keep your
servers in a searchable library with groups, open them in terminal tabs, chain
jump hosts, and manage local and remote port forwards, with secrets stored in
the OS keychain and signed automatic updates.

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

Once installed, Pylentra checks for updates at startup and offers to install
and restart from a banner.

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

## Releasing

One-time setup of the updater signing key:

1. `pnpm tauri signer generate -w ~/.tauri/pylentra.key`. Keep the private key
   and its password safe; never commit them. Losing the key means installed
   copies can no longer auto-update.
2. Paste the printed public key into `plugins.updater.pubkey` in
   `src-tauri/tauri.conf.json` (it ships as a placeholder, and the release
   workflow fails until it is replaced).
3. Add the repository secrets `TAURI_SIGNING_PRIVATE_KEY` (the key file
   content) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Per release: bump `version` in the root `Cargo.toml`, update `CHANGELOG.md`,
walk through [`docs/release-checklist.md`](docs/release-checklist.md), then
push a `vX.Y.Z` tag. The `Release` workflow builds Linux and Windows, creates
a **draft** GitHub release with `latest.json` and `SHA256SUMS.txt`; publish
the draft once the checklist passes.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
