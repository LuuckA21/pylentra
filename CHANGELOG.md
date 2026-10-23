# Changelog

All notable changes to Pylentra are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-10-23

### Changed

- Documentation: `.deb` installs do update from the in-app banner (earlier
  docs said they could not); installing asks for the administrator password
  (`pkexec`, `zenity` or `kdialog`).
- Releases are built when a version bump lands on `main`. Every package's
  updater signature is checked against the app's public key before the
  update manifest (`latest.json`) is published, so a mis-signed build never
  reaches installed apps.

## [0.1.0] - 2026-10-22

### Added

- SSH connection library with groups, search and per-connection settings.
- Terminal tabs (xterm.js) over SSH with password, private key (optional
  passphrase) and SSH agent authentication; saved secrets go to the OS
  keychain.
- Jump host chains (up to 5 hops), local and remote port forwards.
- Host key verification against Pylentra's own `known_hosts` and the read-only
  OpenSSH `~/.ssh/known_hosts`.
- English and Italian UI; light, dark and system themes.
- Signed auto-updates (check on startup, install and restart from a banner).
- Linux (AppImage, `.deb`) and Windows (NSIS installer) release builds with
  `SHA256SUMS.txt`.
