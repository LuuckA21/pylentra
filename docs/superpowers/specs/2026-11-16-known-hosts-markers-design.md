# known_hosts: `@revoked`, wildcard patterns, `@cert-authority` warning (issue #32)

## Intent

Pylentra's goal for 1.0 is to be stable and safe for its existing users.
Pylentra reads `~/.ssh/known_hosts` and the system `ssh_known_hosts` but
never writes them. Today it skips every `@` line, and it matches host names
only exactly or in the hashed `|1|` form. As a result:

- A key the user revoked in OpenSSH is still accepted.
- A key different from the one recorded on a `*.example.com` line gets the
  "unknown key, trust it?" prompt instead of being blocked.

Both are security holes. Security always comes first.

## Goals

1. **`@revoked`.** A revoked key is always refused, on every hop. This holds
   even when Pylentra's own file has it saved or another line accepts it.
   The user never sees a trust prompt for it.
2. **Wildcard patterns.** Lines in the two OpenSSH files that use `*`, `?`
   and `!` behave as in OpenSSH. A matching key connects; a different key is
   blocked as a changed key.
3. **`@cert-authority`.** Explicitly unsupported. When such a line covers the
   host, the trust prompt warns that the host uses SSH certificates and that
   Pylentra does not check them.
4. **Malformed lines.** They are skipped and never hide valid lines, with
   tests.

## Non-goals and anti-goals

- **Non-goal: host certificate support.** Pylentra will not request
  certificates or verify CA signatures. This was approach B, set aside, and
  could come after 1.0.
- **Anti-goal: writing to `~/.ssh` or the system file**, or silently removing
  anything from Pylentra's file.
- **Anti-goal: a way to bypass a revocation from the UI.**

## Approaches

- **A, chosen: close the holes.** Support `@revoked` and patterns, and
  declare `@cert-authority` unsupported with a warning in the prompt. This
  is small, and it fixes every real hole.
- **B: A plus host certificates.** Request a certificate from hosts covered
  by `@cert-authority` and verify its signature, validity, principal and
  revoked CAs. This only helps setups with an SSH CA. It adds sensitive
  verification code, so it is its own project.

The user picked A.

## Decided

- **Where revocations come from.** They are read from all three files:
  Pylentra's, `~/.ssh/known_hosts` and the system file.
- **Revocations are host-scoped, as in OpenSSH.** A `@revoked` line applies
  to the hosts its patterns match, so `@revoked * <key>` covers every host.
  A revoked key that matches the presented key wins over everything else:
  it is checked before any "known" or "unknown" result.
- **The message.** The error overlay that already shows a changed key gets
  a revoked variant. It shows the fingerprint and which file revokes the
  key, with text like "The host key of host:port is revoked in
  `~/.ssh/known_hosts`. The connection was refused." It offers no "forget"
  hint and no bypass: to undo, the user edits the file.
- **Patterns follow OpenSSH `match_hostlist`.** The host field is a comma
  list. Any matching negated `!pattern` excludes the line. Otherwise any
  matching positive pattern includes it. Matching ignores ASCII case and is
  against `host` on port 22 and `[host]:port` otherwise, so
  `[*.example.com]:2222` works. Hashed `|1|` entries stay exact matches.
- **Pattern scope.** Patterns are read only in the two OpenSSH files.
  Pylentra's own file holds only exact names, written by Pylentra.
- **Negotiation.** Keys found through patterns also feed `algorithms()`, so
  their types are preferred in negotiation, as in 0.9.0.
- **The `@cert-authority` warning.** When a `@cert-authority` line in any of
  the read files matches the host, `SessionEvent::HostKeyPrompt` carries a
  flag and the trust prompt adds the warning, in en and it. Suggested text:
  "This host uses SSH certificates, which Pylentra does not check: confirm
  the fingerprint with your administrator before trusting it." Only the
  unknown-key prompt changes. Known and changed keys behave as before.
- **Docs.** The README's "Known limitations" entry for `known_hosts` is
  rewritten: `@revoked` and patterns are honoured, `@cert-authority` is not,
  and the prompt warns. Changes go in the CHANGELOG `### Security` section.
  The release checklist gets one manual step: a revoked key blocks and a
  `@cert-authority` host warns.

## Left to the builder

- **Reuse the glob.** Use the existing iterative `glob` in `ssh_config.rs`,
  which is ASCII case-insensitive and safe against backtracking. Move it to a
  shared place; do not write a second one.
- **Line scanning.** Use one scanner for all three files that yields the
  marker (`Plain`, `Revoked`, `CertAuthority`) plus the hosts, the algorithm
  and the key. `key_lines` becomes that scanner, and `list` and `forget`
  keep their behaviour. The Pylentra-file checks still go through russh,
  except for revocations.
- **New types.** Add a `HostKeyStatus::Revoked { source, fingerprint }`
  status, a new `CoreError` variant with code `hostkey.revoked`, and a new
  session event or overlay data for the revoked case. Pick whichever fits
  `ErrorOverlay` with the least code. Regenerate the bindings.
- **Tests.**
  - A revoked key in each file, including one that Pylentra's file has saved
    and one that another line also accepts.
  - `@revoked *`.
  - Patterns with `*`, `?`, `!` negation and a bracketed port.
  - A pattern match that blocks a changed key.
  - Malformed and marker lines next to valid ones.
  - The `@cert-authority` flag set and not set.
  - UI tests for the revoked overlay and the prompt warning.

## Release

This is a security behaviour change: more connections are refused on
purpose. The proposed version is 0.10.0, to be confirmed with the user.
