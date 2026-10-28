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
   CHANGELOG. In the same build, paste a few KB of text into `cat > /dev/null`
   and type a few characters: they arrive, and the console shows no IPC error.
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
2. Optional (Docker Desktop on Windows may not listen on `::1`): repeat with an
   IPv6 target such as `tester@[::1]:2201`, and without a port as
   `tester@::1`: it parses and connects.
3. Click "Save connection" on the tab: the connection appears in the sidebar
   and the tab now belongs to it.

## Import from ~/.ssh/config

1. With a `~/.ssh/config` (Windows: `%USERPROFILE%\.ssh\config`) that has a
   few hosts, one of them with `ProxyJump`, open Import (the sidebar "⋯" menu,
   or the button in an empty library): the preview lists every host and flags
   the ones it cannot fully import.
2. Import: the hosts land in the chosen destination group, "No group" by
   default; to test the group, choose `New group "ssh config"`. The jump host
   chain works when opened.
3. Run the import again: hosts already in the library are skipped.
4. Import a host with `ProxyJump a,b` (two hops): it lands as a chain of jump
   hosts and opens through both. A chain that conflicts with another jump
   host is skipped, with a note in the preview.

## Snippets

1. Open the palette with `Ctrl+Shift+P` and with the "Snippets" button in the
   status bar. Create a global snippet `ls` and a snippet tied to one
   connection; the second appears only in that connection's tabs.
2. Pick `ls` without "Run immediately": the text is pasted at the prompt and
   nothing runs. With "Run immediately" on, it also runs.
3. A multi-line snippet in bash (bracketed paste): pasted with no
   confirmation. The same in `sh` (no bracketed paste, run `sh` first): a
   confirmation appears first, and Cancel pastes nothing.
4. Select text in the terminal, choose "Save selection as snippet" in the
   palette: the snippet is created with that text.
5. Settings, Snippets: edit and delete a snippet; the changes persist after a
   restart and `snippets.toml` sits next to `connections.toml`.
6. Delete a connection that has snippets: its snippets are gone too, the
   global ones stay.
7. Make `snippets.toml` unreadable (Linux: `chmod 000 snippets.toml`) and
   start the app: it starts, the banner names the file and the reason,
   Settings → Snippets says they are read-only, the file is not modified, and
   deleting a connection still works. Restore the permissions afterwards.

## SOCKS forward

1. On a connection add a SOCKS forward on the default `127.0.0.1:1080` ("+
   SOCKS" in the connection form, or the type "SOCKS (dynamic)" in the
   forward panel), start it, and check the status bar shows "SOCKS :1080".
2. With a service visible only from the server, run
   `curl --socks5-hostname 127.0.0.1:1080 http://internal-service` (or set the
   browser's SOCKS5 proxy): it answers.
3. Stop the forward: the port is released immediately.
4. Bind it to a port already in use: the panel shows an error and the session
   stays open.
5. Import a `~/.ssh/config` host with `DynamicForward 1080`: it lands with a
   SOCKS forward and no "skipped" note.

## Snippet variables

1. Create a snippet `ssh {{servizio}}` and send it: Pylentra asks for
   `servizio` and pastes the text with the value.
2. Send it again: the last value is proposed. Close the app and open it again:
   the value is forgotten, and it is not in `snippets.toml`.
3. `{{{{` pastes as `{{`, and `{{ range }}` or `{{ .Values.x }}` is pasted
   as it is, without a prompt. The palette marks the snippets that ask for
   values with a pill.

## Notes

1. Add a note to a connection (form, under Appearance): a note icon appears in
   the sidebar, and hovering it shows the note.
2. Search the sidebar and Quick connect for a word from the note, and for the
   name of the connection's group: the connection is found.

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

## Split screen

1. Open a tab and split it into a 2x2 grid with Ctrl+Shift+D and Ctrl+Shift+E
   (or the "Split right"/"Split down" buttons in the status bar), choosing a
   different server in each Quick connect picker (Esc cancels a split): four
   panes, each with its own session and status bar. With 4 panes, both split
   buttons and both shortcuts do nothing.
2. Run `htop` in one pane and drag a divider: it redraws to the new size, and
   no pane gets smaller than 160x120 px.
3. Click the Quick connect field at the top and press Tab: the first stops
   are the dividers (a terminal swallows Tab, so they cannot be reached from
   inside one). The arrow keys move the highlighted one in 5% steps, Enter and
   double-click split evenly.
   After dragging a divider with the mouse, typing still reaches the terminal
   that had the keyboard.
4. Alt+Left, Right, Up and Down move the focus between panes (accent border);
   clicking a pane focuses it. In zsh, Alt+B and Alt+F still move by word.
5. Trigger a password prompt in a pane without focus (for example a
   reconnection): it appears in that pane without taking the focus from the
   pane you are typing in; clicking it (or Alt+arrow to that pane) lets you
   answer.
6. With the confirmation setting on (Settings, the one that asks before
   closing live sessions), Ctrl+Shift+W on a pane with a live session asks
   for confirmation, then closes that pane and the others fill the space.
   Closing the last pane closes the tab.
7. With the same setting on, click the tab's x with several live sessions:
   one confirmation for all of them.
8. On Linux, click a status-bar button (Split, Forward): the keyboard ends up
   in the terminal, or in the dialog the button opens, never nowhere.

## Settings

1. Walk through every category in Settings (appearance, terminal, connection,
   security, updates): changes apply right away and survive a restart.
2. Terminal themes Dracula, Nord and Solarized change the open terminals.
3. Updates: "Check now" reports an available version or "up to date"; with
   the update check off, nothing is checked at startup.

## Window

1. Maximize the window, close the app and open it again: it reopens maximized.
2. Resize and move the window, close the app and open it again: same size and
   position.
3. With the window on a second monitor, close the app, unplug that monitor and
   open the app again: the window is visible on the main monitor.
4. F11 enters and leaves fullscreen; after closing in fullscreen, the app
   reopens not in fullscreen.
5. Close the app while in fullscreen, open it again and leave fullscreen: the
   window has its previous normal size, not the monitor size.

## Icon

1. The new app icon shows in the taskbar (or dock), the application menu, the
   window title bar and, on Windows, in the installer.
2. On Windows, after updating over a version with another icon, a pinned
   taskbar icon may stay old until a restart (icon cache): that is expected,
   see "Known limitations" in the README.

## Downgrade to 0.1.x

1. Get a 0.1.x build from GitHub Releases (v0.1.1). Use a 0.2 library that
   includes an Automatic connection (a saved quick connect, or an imported
   host without `IdentityFile`), and back up `connections.toml` first: Linux
   `~/.config/me.luucka.pylentra/`, Windows `%APPDATA%\me.luucka.pylentra\`.
2. Open it in 0.1.x: it opens read-only ("The configuration was written by a
   newer version and is read-only") and, since 0.1.x cannot read the Automatic
   connection, the library shows empty; edits are refused.
3. Go back to 0.2: every connection is there again, and `connections.toml` is
   identical to the backup.

## Downgrade to 0.2.2

1. Get a 0.2.2 build from GitHub Releases. Use a 0.3 library with a note on
   one connection and a few snippets, and back up `connections.toml` and
   `snippets.toml` first (same folders as for 0.1.x above).
2. Open it in 0.2.2: the library opens normally and the connections are there.
3. Save any change in 0.2.2 (edit a connection, for example): every
   connection loses its note, because 0.2.2 rewrites the whole
   `connections.toml`. `snippets.toml` is untouched (0.2.2 ignores it).
4. Go back to 0.3: all the notes are gone, and all the snippets are back.

## Downgrade to 0.3.1

1. Get a 0.3.1 build from GitHub Releases. Use a 0.4 library with a SOCKS
   forward, and back up `connections.toml` first (same folders as for 0.1.x
   above).
2. Open it in 0.3.1: it opens read-only and, since 0.3.1 does not know the
   SOCKS forward, the library shows empty; edits are refused and nothing is
   written.
3. Go back to 0.4: every connection and forward is there, and
   `connections.toml` is identical to the backup.
4. Delete the SOCKS forwards in 0.4 and open the library in 0.3.1 again: the
   connections are shown, still read-only, and nothing is written. Going back
   to 0.4 shows everything.

## Unknown fields

1. Add an invented field by hand to a connection in `connections.toml` (for
   example `futuro = 1`), and to `snippets.toml` and `settings.toml`.
2. Start the app and change something (edit a connection, a snippet and a
   setting): the invented fields are still in the three files.

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
