# known_hosts markers and patterns (0.10.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pylentra honours `@revoked` lines and wildcard host patterns in the
known_hosts files it reads. When a `@cert-authority` line covers a host, the
trust prompt warns that its certificates are not checked.

**Architecture:**
- All the matching lives in `crates/pylentra-core/src/known_hosts.rs`.
- One line scanner, shared by all three files, yields the marker, the host
  field and the key.
- Pattern matching reuses the existing `ssh_config` glob, moved to a small
  shared module.
- A revoked key is a new `HostKeyStatus` that the session handler turns into
  a new event plus error, mirroring the "changed key" path. The UI renders it
  in the existing `ErrorOverlay`.

**Tech Stack:** Rust (russh 0.65.1, ts-rs bindings), Tauri 2, Svelte 5 +
vitest.

**Spec:** `docs/superpowers/specs/2026-11-16-known-hosts-markers-design.md`

## Global Constraints

- `~/.ssh/known_hosts` and the system file are never written; Pylentra never
  removes anything from its own file on its own.
- **A revoked key is always refused, on every hop**, even when it is saved in
  Pylentra's file or accepted by another line. It never prompts and has no UI
  bypass.
- **Revocations are read from all three files** and are host-scoped: a
  `@revoked` line applies to the hosts its field matches, so `*` covers all.
- **Patterns follow OpenSSH `match_hostlist`.**
  - The host field is a comma list.
  - Any matching `!pattern` excludes the line; otherwise any matching
    positive pattern includes it.
  - Matching ignores ASCII case and is done against `host` on port 22,
    `[host]:port` otherwise.
  - Hashed `|1|` entries stay exact.
  - Patterns apply to the two read-only files; Pylentra's file keeps exact
    names (russh).
- `@cert-authority` is not supported. Only the unknown-key prompt changes.
- The en warning is: "This host uses SSH certificates, which Pylentra does not
  check: confirm the fingerprint with your administrator before trusting it."
- Version **0.10.0**. Never log paths or host names. i18n is en and it, in
  natural Italian.
- **Checks for every task**, with
  `CARGO_TARGET_DIR=target CARGO_INCREMENTAL=0`:
  - `cargo fmt --all --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace`;
  - `git diff --exit-code ui/src/lib/bindings` after the tests (commit
    regenerated bindings);
  - `pnpm --dir ui lint`, `pnpm --dir ui check` and `pnpm --dir ui test` when
    UI files change.
- Commit trailers (exactly):
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01TAKHhRWxSMZkLTpSd4MtRc`.

## Review Focus

1. **Mixed-case host field** (`*.Example.COM`): it matches `a.example.com`,
   as in OpenSSH. Pinned in Task 1.
2. **A field made only of negations** (`!bad.example.com <key>`) matches no
   host at all; it must not turn into "every other host". Pinned in Task 1.
3. **A bare `*` and a non-standard port:** `*` matches `[h]:2222`, while
   `*.example.com` does not match `[a.example.com]:2222`. Pinned in Task 1.
4. **A `@revoked` line whose key does not parse** (an unsupported type, or
   broken base64) is skipped with a warning. It neither blocks every host
   nor errors the check. Pinned in Task 2.
5. **Auto-reconnect never retries a revoked-key failure**: `shouldReconnect`
   is an allowlist, but pin `hostkey.revoked` there. Pinned in Task 4.

---

### Task 1: Shared glob and pattern matching for read-only files

**Files:**
- Create: `crates/pylentra-core/src/pattern.rs`
- Modify: `crates/pylentra-core/src/lib.rs` (add `pub(crate) mod pattern;`)
- Modify: `crates/pylentra-core/src/ssh_config.rs:107-146`. Move `glob` out;
  `block_matches` calls `pattern::list_matches`.
- Modify: `crates/pylentra-core/src/known_hosts.rs`: `key_lines`,
  `host_matches`, `recorded_keys` and the tests.

**Interfaces:**
- Produces:
  - `pub(crate) fn glob(pattern: &[u8], s: &[u8]) -> bool` (moved unchanged,
    with its comment).
  - `pub(crate) fn list_matches<'a>(patterns: impl IntoIterator<Item = &'a str>, s: &str) -> bool`.
    It carries the semantics of today's `block_matches`: a matching `!neg`
    gives false, otherwise true if any positive pattern matches.
  - In `known_hosts.rs`:
    - `#[derive(Clone, Copy, PartialEq, Debug)] enum Marker { Plain, Revoked, CertAuthority }`.
    - `fn key_lines(text: &str) -> impl Iterator<Item = (usize, Marker, &str, &str, &str)>`
      yields (line, marker, hosts, algo, base64). It skips `#` lines, unknown
      `@` markers and lines with too few fields.
    - `fn host_field_matches(host_port: &str, field: &str) -> bool`. Hashed
      `|1|` entries are matched exactly through the existing HMAC code; every
      other entry goes through `pattern::list_matches` over
      `field.split(',')`.
- `list` and `forget` keep their behaviour: they use only `Marker::Plain`
  lines, and `list` still splits names as today.

- [ ] **Step 1: Write the failing tests** in `known_hosts.rs` `mod tests`.
  They use the existing `setup()` and `key()`, and write
  `ssh_known_hosts`:
  - `wildcard_line_is_known_and_blocks_other_keys`: for
    `*.example.com {pk}`:
    - `check("a.example.com", 22, &pk_key)` is `Known`;
    - `check("a.example.com", 22, &other)` is `Mismatch { source: OpenSsh, type_changed: false, .. }`.
  - `question_mark_and_mixed_case`: `web?.Example.COM {pk}` gives `Known`
    for `web1.example.com`. `web10.example.com` with another key gives
    `Unknown`.
  - `negation_excludes_a_host`: with `*.example.com,!bad.example.com {pk}`,
    `bad.example.com` with another key gives `Unknown`, and
    `good.example.com` with another key gives `Mismatch`.
  - `negation_only_field_matches_nothing`: `!bad.example.com {pk}` gives
    `Unknown` for `x.example.com` with another key and for
    `bad.example.com` with `pk`.
  - `bracketed_port_patterns`:
    - `[*.example.com]:2222 {pk}` gives `Known` for
      `("a.example.com", 2222)` with `pk`.
    - With only `*.example.com {pk}`, `("a.example.com", 2222)` with another
      key gives `Unknown`.
    - With only `* {pk}`, `("h", 2222)` with another key gives `Mismatch`.
  - `algorithms_include_pattern_lines`: with `*.example.com {ecdsa_pk}`,
    `kh.algorithms("a.example.com", 22)` contains the ECDSA algorithm. Use
    the existing `ecdsa()` helper.
  - Update `openssh_file_noise_does_not_error`: `a.example.com` with `k`
    under `*.example.com {other}` is now `Mismatch { source: OpenSsh, .. }`.
    Keep the rest.

- [ ] **Step 2: Run them and see them fail**
  Run: `cargo test -p pylentra-core known_hosts::tests`
  Expected: the new tests FAIL (patterns are ignored today, so the result is
  `Unknown`).

- [ ] **Step 3: Implement** the interfaces above.
  - `recorded_keys` uses `key_lines` with `Marker::Plain` and
    `host_field_matches`.
  - Lowercasing is done by `glob`, which ignores ASCII case. The host passed
    in is already lowercase.

- [ ] **Step 4: Run all checks** from Global Constraints. Expected: PASS,
  including the unchanged `tests/ssh_config.rs` (`block_matches` behaviour is
  identical).

- [ ] **Step 5: Commit** `known_hosts: honour wildcard and negated host patterns` (body: `Part of #32.` + trailers).

### Task 2: `@revoked` blocks the key everywhere (core)

**Files:**
- Modify: `crates/pylentra-core/src/known_hosts.rs`: `HostKeyStatus`,
  `check`, a new `revoked_by` and the tests.
- Modify: `crates/pylentra-core/src/error.rs`: the `HostKeyRevoked` variant,
  `code()` and `to_api()`.
- Modify: `crates/pylentra-core/src/ssh/events.rs`: `SessionEvent::HostKeyRevoked`.
- Modify: `crates/pylentra-core/src/ssh/handler.rs:36`: handle `Revoked`.
- Modify: `ui/src/lib/i18n/codes.ts`, `en.ts` and `it.ts`: add the
  `error.hostkey.revoked` key.
- Regenerate the bindings: `HostKeyStatus` is not exported;
  `SessionEvent.ts` changes.

**Interfaces:**
- Consumes: `key_lines`, `Marker` and `host_field_matches` from Task 1.
- Produces:
  - `HostKeyStatus::Revoked { source: KeySource, fingerprint: String }`.
  - `CoreError::HostKeyRevoked { host: String, port: u16 }`: code
    `"hostkey.revoked"`, params `host` and `port`, Display
    `"host key of {host}:{port} is revoked"`.
  - `SessionEvent::HostKeyRevoked { host: String, port: u16, fingerprint: String, source: KeySource }`.
  - en `error.hostkey.revoked`: `"The host key of {host}:{port} is revoked. The connection was refused."`

- [ ] **Step 1: Write the failing tests** in `known_hosts.rs` `mod tests`:
  - `revoked_in_openssh_file_wins_over_pylentra`:
    - `kh.learn("h", 22, &k)`;
    - `ssh_known_hosts` gets `@revoked h {k}`;
    - `check("h", 22, &k)` is `Revoked { source: OpenSsh, fingerprint: fingerprint(&k) }`.
  - `revoked_wins_over_another_accepting_line`: with `h {k}\n@revoked h {k}`
    in `ssh_known_hosts`, the result is `Revoked { source: OpenSsh, .. }`.
  - `revoked_in_pylentra_file`: Pylentra's file holds `@revoked h {k}` (hand
    edit), and the result is `Revoked { source: Pylentra, .. }`.
  - `revoked_in_system_file`: use `with_system`; the result is
    `Revoked { source: System, .. }`.
  - `revoked_star_covers_every_host_and_port`: with `@revoked * {k}`, both
    `("a", 22)` and `("b", 2222)` with `k` are `Revoked`.
  - `revoked_hashed_host`: `@revoked |1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= {k}`
    gives `Revoked` for `"example.com"` (the existing hashed test vector).
  - `revoked_other_key_does_not_block`: `@revoked h {other}` with `k` gives
    `Unknown`.
  - `revoked_line_with_unparseable_key_is_skipped`:
    `@revoked * ssh-ed25519 !!!` followed by `h {k}` gives `Known` and no
    error.
  - In `error.rs` tests (or the existing code table test, if there is one):
    `CoreError::HostKeyRevoked { host: "h".into(), port: 22 }.code() == "hostkey.revoked"`.

- [ ] **Step 2: Run them and see them fail**
  Run: `cargo test -p pylentra-core known_hosts::tests::revoked`
  Expected: FAIL to compile (no `Revoked` variant).

- [ ] **Step 3: Implement**
  - Add `fn revoked_by(&self, host: &str, port: u16, key: &PublicKey) -> Result<Option<KeySource>, CoreError>`.
    - It scans Pylentra's file, then the read-only files, for
      `Marker::Revoked` lines whose field matches and whose key parses equal
      to `key`.
    - Reading follows `recorded_keys`' rules: a missing file is empty, and
      only the System file tolerates PermissionDenied. Factor the read into
      one shared `fn read_text(source, path) -> Result<Option<String>, CoreError>`
      instead of copying it.
  - `check` calls `revoked_by` first and returns `Revoked`, before russh is
    consulted.
  - In `handler.rs`:
    - On `Revoked`, emit `SessionEvent::HostKeyRevoked` and return
      `Err(CoreError::HostKeyRevoked { .. })`.
    - Add the i18n key in en and it. it: `"La chiave host di {host}:{port} è revocata. Connessione rifiutata."`
    - Add the code to `CORE_CODES`.

- [ ] **Step 4: Run all checks.** Expected: PASS. `SessionEvent.ts` is
  regenerated; commit it.

- [ ] **Step 5: Commit** `known_hosts: refuse revoked host keys in every file` (body `Part of #32.` + trailers).

### Task 3: `@cert-authority` warning in the trust prompt

**Files:**
- Modify: `crates/pylentra-core/src/known_hosts.rs`: add `cert_authority()`
  and tests.
- Modify: `crates/pylentra-core/src/ssh/events.rs`: add a field to
  `HostKeyPrompt`.
- Modify: `crates/pylentra-core/src/ssh/handler.rs:56-62`.
- Modify: `ui/src/components/HostKeyDialog.svelte`, `en.ts` and `it.ts`; add
  a test next to the dialog's existing tests (`PaneArea.test.ts` already
  renders `HostKeyPrompt`; add one there or in a new
  `HostKeyDialog.test.ts`, following the existing component tests).

**Interfaces:**
- Consumes: `key_lines`, `Marker::CertAuthority`, `host_field_matches` and
  `read_text` from Tasks 1–2.
- Produces:
  - `pub fn cert_authority(&self, host: &str, port: u16) -> Result<bool, CoreError>`:
    true if any of the three files has a `@cert-authority` line whose field
    matches. The key is not parsed.
  - `SessionEvent::HostKeyPrompt { .., cert_authority: bool }`.
  - en `hostkey.certAuthority`: `"This host uses SSH certificates, which Pylentra does not check: confirm the fingerprint with your administrator before trusting it."`
  - it `hostkey.certAuthority`: `"Questo host usa certificati SSH, che Pylentra non controlla: verifica l'impronta con l'amministratore prima di fidarti."`

- [ ] **Step 1: Write the failing tests**
  - `cert_authority_flag`:
    - with `@cert-authority *.corp.example ssh-ed25519 AAAA…{k}` in
      `ssh_known_hosts`, `cert_authority("a.corp.example", 22)` is true and
      `cert_authority("other.example", 22)` is false;
    - with no files, the result is false.
  - UI tests:
    - a `HostKeyPrompt` with `cert_authority: true` renders the
      `hostkey.certAuthority` text, and the text is in the dialog's
      `aria-describedby`;
    - with `cert_authority: false`, the text is absent.

- [ ] **Step 2: Run them and see them fail**
  Run: `cargo test -p pylentra-core cert_authority` and `pnpm --dir ui test`.
  Expected: FAIL (no method, no field).

- [ ] **Step 3: Implement.**
  - The handler sets the flag only on the Unknown path. Get it from
    `known_hosts.cert_authority(...)`; if that read fails, set false and log
    without a path. The prompt must not fail because of a warning.
  - In the dialog, add a `<p id="{uid}-ca">` under the body when the flag is
    set, and add it to `aria-describedby`.

- [ ] **Step 4: Run all checks.** Expected: PASS, with `SessionEvent.ts`
  regenerated and committed.

- [ ] **Step 5: Commit** `known_hosts: warn in the trust prompt when a @cert-authority line covers the host` (body `Part of #32.` + trailers).

### Task 4: Revoked key in the error overlay

**Files:**
- Modify: `ui/src/lib/sessionReducer.ts`: add
  `revoked?: Extract<SessionEvent, { type: "HostKeyRevoked" }>`, stored like
  `mismatch`.
- Modify: `ui/src/components/ErrorOverlay.svelte` and
  `ui/src/components/TerminalPane.svelte:603`: pass `revoked={pane.revoked}`.
- Modify: `en.ts` and `it.ts`.
- Test: `ui/src/lib/sessionReducer.test.ts`, `ui/src/lib/reconnect.test.ts`,
  and the overlay test (see Step 1).

**Interfaces:**
- Consumes: `SessionEvent::HostKeyRevoked` from Task 2.
- Produces (i18n):
  - `session.revoked` (label): en `"Revoked fingerprint"`, it
    `"Impronta revocata"`.
  - `session.revokedOpenSsh`: en `"The key is revoked in your OpenSSH known_hosts (~/.ssh/known_hosts). To accept it again, remove its @revoked line from that file."`,
    it `"La chiave è revocata nel known_hosts di OpenSSH (~/.ssh/known_hosts). Per accettarla di nuovo, togli la sua riga @revoked da quel file."`.
  - `session.revokedSystem`: en `"The key is revoked in the system-wide ssh_known_hosts, which Pylentra only reads. Ask the administrator."`,
    it `"La chiave è revocata nel file ssh_known_hosts di sistema, che Pylentra legge soltanto. Rivolgiti all'amministratore."`.
  - `session.revokedPylentra`: en `"The key is revoked in Pylentra's own known_hosts. To accept it again, remove its @revoked line from that file."`,
    it `"La chiave è revocata nel known_hosts di Pylentra. Per accettarla di nuovo, togli la sua riga @revoked da quel file."`.

- [ ] **Step 1: Write the failing tests**
  - Reducer: `HostKeyRevoked` is stored in `revoked`, and the phase follows
    the same rules as `mismatch`. Mirror the existing
    `mismatch stored and phase closed on Closed` test.
  - `reconnect.test.ts`: add `"hostkey.revoked"` to the `test.each` list of
    codes that never reconnect.
  - Overlay: render `ErrorOverlay` with a `revoked` event for each `source`.
    Check that:
    - the fingerprint and the matching hint are shown;
    - neither the "Forget host" button nor the `ssh-keygen -R` hint appears
      for any source.
    Use the existing component-test style (`@testing-library/svelte`, as in
    the other `*.test.ts`).

- [ ] **Step 2: Run them and see them fail**
  Run: `pnpm --dir ui test`. Expected: the new tests FAIL.

- [ ] **Step 3: Implement.** Add a `{#if revoked}` block next to the
  `mismatch` one: a `<dl>` with `session.revoked` and the fingerprint, plus
  one hint chosen by `source`. No new button.

- [ ] **Step 4: Run all checks (UI ones included).** Expected: PASS.

- [ ] **Step 5: Commit** `ui: show revoked host keys in the error overlay` (body `Part of #32.` + trailers).

### Task 5: Docs, version 0.10.0

**Files:**
- Modify: `Cargo.toml` (`version = "0.10.0"` in `[workspace.package]`) and
  `Cargo.lock` (via `cargo check`).
- Modify: `CHANGELOG.md`, adding
  `## [0.10.0] - <release date>` above `[0.9.0]`. Open it with a one-line
  intro: "Nothing saved changes format: going back to 0.9.x loses nothing."
  Then a `### Security` section with:
  - `@revoked` keys are refused in all three files and on every hop, even if
    saved in Pylentra;
  - wildcard and negated patterns in the OpenSSH files are honoured, so a
    different key is blocked as changed;
  - the trust prompt warns when a `@cert-authority` line covers the host
    (certificates are not checked).
- Modify: `README.md:62-67`. The `known_hosts` limitation becomes:
  `@cert-authority` lines (host certificates) are not supported, and the
  trust prompt warns about them. Keep the existing `HostKeyAlias` /
  `UserKnownHostsFile` sentence.
- Modify: `docs/release-checklist.md`, in the "Security checks" section. Add
  one step: a key put under `@revoked * <key>` in `~/.ssh/known_hosts`
  blocks with the revoked overlay, and a host covered by `@cert-authority`
  shows the warning in the trust prompt.

- [ ] **Step 1: Make the edits** above.
- [ ] **Step 2: Run all checks.** Expected: PASS.
- [ ] **Step 3: Commit** `Release 0.10.0: known_hosts @revoked and patterns` (body `Closes #32.` + trailers).
