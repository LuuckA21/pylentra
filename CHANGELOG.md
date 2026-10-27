# Changelog

All notable changes to Pylentra are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.4.0] - 2026-10-27

Going back to 0.3.x: `connections.toml` is now at format version 3, so 0.3.x
opens it read-only. If the library only has rules 0.3.x knows, it shows it; if
it has a SOCKS forward, it shows an empty library. Either way nothing is
written, and nothing is lost: going back to 0.4 shows everything again.
`snippets.toml` and `settings.toml` keep their format; 0.3.x pastes snippet
variables as plain text.

### Added

- Dynamic (SOCKS5) port forwarding, the third forward type, like `ssh -D`:
  add it with "+ SOCKS" in the connection form, or pick "SOCKS (dynamic)" as
  the type in the forward panel. It listens on `127.0.0.1:1080` by default,
  and the server resolves the destination. It speaks SOCKS5 without
  authentication and only CONNECT. The status bar shows a "SOCKS :port" pill
  while it runs. For example:
  `curl --socks5-hostname 127.0.0.1:1080 http://internal-service`.
- Importing from `~/.ssh/config` turns `DynamicForward` lines into SOCKS
  forwards (before, they were skipped with a note).
- Snippet variables: `{{name}}` in a snippet asks for the value when the
  snippet is sent, with one single-line field per variable. The last value is
  proposed again while the app stays open and is never written to disk. Write
  `{{{{` for a literal `{{`; any other text between `{{ }}` (for example Helm
  templates) is left as it is. A pill in the palette marks the snippets that
  ask for values.
- F11 toggles fullscreen. It is not remembered when the app reopens.

### Changed

- `connections.toml` moved to format version 3 (see above).
- Fields in `connections.toml`, `snippets.toml` and `settings.toml` that a
  future version adds are kept when Pylentra saves. This does not apply
  inside individual forward rules or the authentication method.

## [0.3.1] - 2026-10-25

### Fixed

- An unreadable `snippets.toml` (for example, no permission to read it, or a
  damaged file that cannot be moved aside) no longer stops Pylentra from
  starting. The app starts with no snippets, leaves the file untouched, and
  the startup banner says why.
- The snippet palette closes when you switch to another tab, and keeps a valid
  selection when the snippets change while it is open (Enter used to do
  nothing).
- Deleting a connection no longer shows an error when only reloading the
  snippets afterwards fails: the connection was deleted.
- Importing from `~/.ssh/config`: a jump host shared by several `ProxyJump`
  chains lists its "set by the chain of" notes in the same order whatever the
  order of the hosts in the file.

### Changed

- Documentation: after an update that changes the app icon, Windows may keep
  showing the old icon on a pinned taskbar shortcut until it refreshes its
  icon cache (restart, or `ie4uinit.exe -show`).

## [0.3.0] - 2026-10-25

Going back to 0.2.2: the library opens normally, but any change saved there
removes the notes of every connection. Snippets stay in `snippets.toml`, which
0.2.2 ignores.

### Added

- Snippets: saved commands, global or tied to one connection. Open the palette
  with Ctrl+Shift+P or the "Snippets" button in the status bar; a snippet is
  pasted into the terminal, and with "Run immediately" it also sends Enter.
  Before pasting several lines into a shell that does not support bracketed
  paste, Pylentra asks for confirmation. "Save selection as snippet" in the
  palette turns the selected terminal text into a snippet. Manage them in
  Settings → Snippets; they are stored in `snippets.toml`, next to
  `connections.toml`, and deleting a connection deletes its snippets.
- A note for each connection (a field in the connection form, under
  Appearance). A note icon in the sidebar shows it on hover.
- Importing from `~/.ssh/config` understands `ProxyJump a,b` with several
  hops and turns it into a chain of jump hosts. A chain that conflicts with
  another jump host is skipped, with a note.
- The window reopens maximized if it was maximized, otherwise with the size and
  position it had before.
- A new app icon.

### Changed

- Searching the sidebar, and the Quick connect suggestions, also matches notes
  and the group name.

## [0.2.2] - 2026-10-25

### Fixed

- Importing from `~/.ssh/config` now says when a jump host is removed because
  the jump hosts form a loop or a chain of more than 5 hops (the host then
  connects directly). Before, the jump was dropped silently.
- The password prompt of a quick connect tab names the user
  (`user@host:port`), like saved connections do.
- A hand-edited `connections.toml` or `settings.toml` with a non-text color,
  icon or theme (for example `color = 5`) no longer makes the whole file
  unreadable: that value is ignored.
- Keyboard focus: the forward panel takes focus when you open it and gives it
  back when you close it, and the Forward button reports whether it is open.
  A panel opened by a failing forward no longer takes focus, and opens only
  once per failure, so closing it sticks. On narrow windows, opening the
  sidebar moves focus to search and Escape returns it to the Expand button;
  widening the window closes the overlay.

## [0.2.1] - 2026-10-25

The first published 0.2 release: 0.2.0 was never released. Everything new in
0.2 (quick connect, import from `~/.ssh/config`, colors and icons, settings by
category, the new look) is listed under
[0.2.0](https://github.com/LuuckA21/pylentra/blob/main/CHANGELOG.md#020---2026-10-24).

### Fixed

- "Edit" in a connection's menu did nothing; it opens the connection form
  again.
- The settings button showed a sun-like icon that looked like a theme toggle;
  it now shows sliders.
- The "New connection" button no longer wraps onto two lines.

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

### Security

- Updated the `crypto-bigint` crypto dependency (via `russh`) to 0.7.8 for
  [RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387)
  (timing variability in operations meant to be constant-time).

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
