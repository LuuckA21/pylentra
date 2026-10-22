#!/usr/bin/env bash
# Verifies a Tauri updater signature (<file>.sig, as written by
# `tauri signer sign`) against plugins.updater.pubkey in tauri.conf.json.
# Tauri stores minisign files base64-encoded; minisign does the check.
# Usage: scripts/verify-updater-sig.sh <file> [<file.sig>]
set -euo pipefail
file=$1
sig=${2:-$1.sig}
root=$(cd "$(dirname "$0")/.." && pwd)
pubkey=$(jq -r .plugins.updater.pubkey "$root/src-tauri/tauri.conf.json" | base64 -d | tr -d '\r' | sed -n 2p)
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
base64 -d < "$sig" | tr -d '\r' > "$tmp"
minisign -V -q -P "$pubkey" -m "$file" -x "$tmp"
