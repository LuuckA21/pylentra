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

Once installed, Pylentra checks for updates at startup. AppImage and Windows
installs get a banner offering to install and restart. **`.deb` installs do
not auto-update**: install the new `.deb` over the old one.

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
- **Auto-update covers the AppImage and the Windows installer only.** `.deb`
  users update manually by installing the new package (`sudo apt install
./Pylentra_*_amd64.deb`); a banner shown there cannot install the update.
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

Without Node: the key is a plain [minisign](https://github.com/jedisct1/minisign)
key pair, base64-encoded. Run `minisign -G -p pylentra.pub -s pylentra.key`,
then use the base64 of `pylentra.pub` as `pubkey` and the base64 of
`pylentra.key` as `TAURI_SIGNING_PRIVATE_KEY` (PowerShell:
`[Convert]::ToBase64String([IO.File]::ReadAllBytes("pylentra.key"))`;
Linux: `base64 -w0 pylentra.key`).

The current public key is minisign key ID `590862C6663206ED`.

Per release: bump `version` in the root `Cargo.toml`, update `CHANGELOG.md`,
walk through [`docs/release-checklist.md`](docs/release-checklist.md), then
start the `Release` workflow on `main` (Actions → Release → Run workflow), or
push a `vX.Y.Z` tag. It builds Linux and Windows and creates a **draft**
GitHub release `vX.Y.Z` (from the workspace version) with `latest.json` and
`SHA256SUMS.txt`; publishing the draft creates the tag. A manual run stops if
that tag already exists.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
