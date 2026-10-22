# Release checklist

Repeat every section below on **Linux** and on **Windows** before publishing
the draft release. They need a display and real installers, so CI does not
cover them.

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
5. In the devtools console, confirm the `data` channel delivers an
   `ArrayBuffer` (spec §13.3). If it delivers an array of numbers instead,
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
4. `.deb` (Linux): auto-update is not supported. Expected result: update by
   installing the new `.deb` manually; if the banner appears, installing from
   it must not corrupt the install. Check whether `latest.json` in the release
   contains a `linux-x86_64-deb` entry (if it does, the banner may trigger a
   pkexec / dpkg prompt; record the behaviour in the CHANGELOG).
5. Offline start: no banner and no error shown.
6. Download the assets and verify them with `sha256sum -c SHA256SUMS.txt`.
