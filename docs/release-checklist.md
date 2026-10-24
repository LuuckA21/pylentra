# Release checklist

Repeat every section below on **Linux** and on **Windows** before publishing
the draft release. They need a display and real installers, so CI does not
cover them.

No build tools needed: every PR's **Preview build** workflow uploads debug
bundles with devtools (`pylentra-preview-Linux`: AppImage and `.deb`;
`pylentra-preview-Windows`: NSIS installer) as run artifacts. They are the
`pnpm tauri build --debug` builds step 5 asks for. Windows shows a SmartScreen
warning (unsigned): "More info → Run anyway".

## Terminal smoke test

Run on each platform before a release; it needs a display, so CI does not
cover it.

1. Start the test servers and the app:

   ```sh
   docker compose -f crates/pylentra-core/tests/compat/docker-compose.yml up -d --build
   pnpm tauri dev
   ```

2. Create a connection to `127.0.0.1:2201`, user `tester`, password
   authentication, and open it (double click or Enter in the sidebar).
3. Check the host key prompt shows the fingerprint, accept it, then enter the
   password `tester` (the server only allows keyboard-interactive, so the
   password goes through that fallback).
4. In the shell:
   - run `htop`, then quit with `q`;
   - run `vim`, type some text, quit with `:q!`;
   - resize the window: `htop` and `vim` redraw at the new size;
   - select text and copy it with `Ctrl+Shift+C`, paste it with
     `Ctrl+Shift+V`; `Ctrl+C` still interrupts a running `sleep 100`;
   - run `exit`: the tab shows "Session ended. Press Enter to reconnect.";
     press Enter and the session reconnects in the same tab (no host key
     prompt this time).
5. On a release build, not `tauri dev` (which serves the page from the Vite
   dev server instead of the bundled assets): build with
   `pnpm tauri build --debug`, which embeds `ui/dist` behind the production CSP
   like `pnpm tauri build` but keeps devtools. Open a session, print more than 1 KB
   (`seq 1 1000`) and confirm in the devtools console that the `data` channel
   delivers an `ArrayBuffer` (spec §13.3). If it delivers an array of numbers instead,
   apply the spec's fallback (base64 on the event channel) and record it in the
   CHANGELOG.
6. Stop the servers:

   ```sh
   docker compose -f crates/pylentra-core/tests/compat/docker-compose.yml down
   ```

## Jump host

1. Create a connection `bastion` to `127.0.0.1:2201` (user `tester`) and a
   connection `target` to the host behind it, with `bastion` as jump host.
2. Open `target`: one host key prompt per hop, then a shell on the target.
3. Delete `bastion`: the confirmation lists `target` as a dependent that
   will lose its jump host.

## Port forwards

1. Local forward: on a connection add a local forward from
   `127.0.0.1:8080` to a service reachable from the server, start it from the
   port forwarding panel, and check `curl http://127.0.0.1:8080` reaches it.
2. Remote forward: add one that listens on the server and points to a local
   service, start it, and check the server can reach the local service.
3. Stop each forward: the port is released immediately.
4. Bind a local forward to a port already in use: the panel shows an error and
   the session stays open.

## Keychain

1. Save a connection with a password and tick "Save password in the keychain": reconnect, no
   prompt.
2. Linux with a Secret Service (GNOME Keyring / KWallet): the entry
   `me.luucka.pylentra` appears in the keychain. Windows: it appears in
   Credential Manager.
3. Linux without a Secret Service: the password is asked at every connection
   and the app does not crash.
4. Delete the connection: the keychain entry is removed.

## Quick connect

1. Type `tester@127.0.0.1:2201` in the Quick connect field and press Enter: the
   session opens without saving a connection (Auto authentication: agent, then
   default keys, then the password prompt).
2. Repeat with an IPv6 target such as `tester@[::1]:2201` (and `::1` without a
   port): it parses and connects.
3. Click "Save connection" on the tab: the connection appears in the sidebar
   and the tab now belongs to it.

## Import from ~/.ssh/config

1. With a `~/.ssh/config` that has a few hosts, one of them with `ProxyJump`,
   open Import: the preview lists every host and flags the ones it cannot
   fully import.
2. Import: the hosts land in the "ssh config" group and the jump host chain
   works when opened.
3. Run the import again: hosts already in the library are skipped.

## Colors and icons

1. Set an icon and a color on a connection and a color on a group: the sidebar
   and the tab show them in both light and dark themes.
2. Connections without a color or icon (imported, quick connect) still look
   right.

## Automatic reconnection

1. Settings, Connections: turn on automatic reconnection and open a session.
2. Drop the network (disable it, or stop the server): the tab shows
   "Reconnecting... attempt N of M"; restore the network and the session comes
   back by itself.
3. Drop it again and click Cancel: no further attempt is made, and Enter
   reconnects by hand.

## Closing live sessions

1. With the confirmation setting on, close a tab with a live session: a
   dialog asks first. Closing the window with live sessions asks as well.
2. Turn the setting off: neither asks.

## Settings

1. Walk through every category in Settings (appearance, terminal, connection,
   security, updates): changes apply right away and survive a restart.
2. Terminal themes Dracula, Nord and Solarized change the open terminals.
3. Updates: "Check now" reports an available version or "up to date"; with
   the update check off, nothing is checked at startup.

## Downgrade to 0.1.x

1. With a library saved by 0.2, open it in 0.1.x: it opens read-only ("The configuration was
   written by a newer version and is read-only"); edits are refused.
2. Go back to 0.2: `connections.toml` is unchanged.

## Language

1. Fresh profile with the system language set to Italian: the UI starts in
   Italian. With any other language: English.
2. Settings: set Language to English / Italiano, every visible string changes without a restart.

## Update from the previous version

1. Install the previous release (AppImage / `.deb` on Linux, installer on
   Windows) and keep some connections saved.
2. Publish the new release (un-draft it) and start the old version: the
   "Version X available" banner appears.
3. Click "Install and restart": the app restarts on the new version
   (Windows installs in passive mode) and the connections are still there.
4. `.deb` (Linux): "Install and restart" asks for the administrator password
   (pkexec / zenity / kdialog) and installs the new package; the app restarts
   on the new version. Cancelling the password prompt leaves the old version
   working.
5. Offline start: no banner and no error shown.
6. Download the assets and verify them with `sha256sum -c SHA256SUMS.txt`.
