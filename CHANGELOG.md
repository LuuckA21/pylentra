# Changelog

All notable changes to Pylentra are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
