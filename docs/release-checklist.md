# Release checklist

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
