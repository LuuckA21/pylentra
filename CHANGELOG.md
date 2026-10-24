# Changelog

All notable changes to Pylentra are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-10-24

### Added

- Quick connect: type `user@host` (or `user@host:port`, IPv6 included) in the
  Quick connect field and press Enter to open a session without saving
  anything, with suggestions from your library and a "Save connection" button on the tab. The
  new Auto authentication tries the SSH agent, then your default keys, then asks
  for a password.
- Import connections from `~/.ssh/config`, with a preview before anything is
  saved. Jump hosts, ports, identity files and local forwards are carried over;
  hosts you already have are skipped.
- Per-connection icon and color, and a color for each group, so servers are easy
  to tell apart in the sidebar.
- Settings by category: terminal themes (Dracula, Nord, Solarized), cursor,
  scrollback and copy on select; default user and port, connect timeout and
  keepalive; automatic reconnection when a session drops; a list of the known
  hosts Pylentra saved; confirmation before closing live sessions or the window;
  and a toggle for the update check, plus a "Check now" button.
- A welcome screen with your recent connections.
- The sidebar collapses on narrow windows.

### Changed

- A new look: rounded panels, layered surfaces and the Manrope font, in both
  the light and dark themes.
- The connection library file (`connections.toml`) moves to format version 2.
  Pylentra 0.1.x opens a 0.2 library read-only, so going back to 0.1 never
  overwrites it. If the library holds a connection with the new Automatic
  authentication (quick connect saves, imported hosts without `IdentityFile`),
  0.1.x cannot read it and shows an empty read-only library: nothing is lost,
  and 0.2 shows everything again.

## [0.1.1] - 2026-10-23

### Changed

- Documentation: `.deb` installs do update from the in-app banner (earlier
  docs said they could not); installing asks for the administrator password
  (`pkexec`, `zenity` or `kdialog`).
- Releases are built when a version bump lands on `main`. Every package's
  updater signature is checked against the app's public key before the
  update manifest (`latest.json`) is published, so a mis-signed build never
  reaches installed apps.

### Security

- Updated the `ctutils` crypto dependency (via `russh`) to 0.4.5 for
  [RUSTSEC-2026-0385](https://rustsec.org/advisories/RUSTSEC-2026-0385)
  (non-constant-time mask generation and `u128` comparisons).

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
