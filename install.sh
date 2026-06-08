#!/usr/bin/env sh
# cc-session installer.
#
# Usage:
#   curl -fsSL https://get-claude-code-session-editor.harshiitkgp.in/install.sh | sh
#
# Env overrides:
#   CC_SESSION_VERSION  - tag to install (default: latest)
#   CC_SESSION_INSTALL_DIR - where to drop the binary
#       (default: /usr/local/bin if writable, else $HOME/.local/bin)
#
# This script downloads a prebuilt release binary for your platform from
#   https://github.com/dudegladiator/claude-code-session-editor/releases
# and installs it as `cc-session`.

set -eu

REPO="dudegladiator/claude-code-session-editor"
BIN="cc-session"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }
note() { printf '%s\n' "$1"; }

# --- detect platform ---
uname_s=$(uname -s 2>/dev/null || echo unknown)
uname_m=$(uname -m 2>/dev/null || echo unknown)

case "$uname_s" in
  Darwin)
    case "$uname_m" in
      arm64|aarch64) target="aarch64-apple-darwin" ;;
      x86_64)        target="x86_64-apple-darwin"  ;;
      *) err "unsupported macOS architecture: $uname_m" ;;
    esac
    ;;
  Linux)
    case "$uname_m" in
      x86_64) target="x86_64-unknown-linux-gnu" ;;
      *) err "unsupported Linux architecture: $uname_m (file an issue at https://github.com/${REPO}/issues)" ;;
    esac
    ;;
  *)
    err "unsupported OS: $uname_s. Try 'cargo install cc-session' instead."
    ;;
esac

# --- pick install dir ---
if [ -n "${CC_SESSION_INSTALL_DIR:-}" ]; then
  install_dir="$CC_SESSION_INSTALL_DIR"
elif [ -w /usr/local/bin ] 2>/dev/null; then
  install_dir="/usr/local/bin"
else
  install_dir="$HOME/.local/bin"
fi
mkdir -p "$install_dir"

# --- pick version ---
if [ -n "${CC_SESSION_VERSION:-}" ]; then
  tag="$CC_SESSION_VERSION"
else
  # GitHub redirects /latest -> /tag/<vX>; resolve via header.
  tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest" \
    | sed -E 's|.*/tag/(v[^/]+).*|\1|')
  [ -n "$tag" ] || err "could not resolve latest release tag"
fi

asset="${BIN}-${target}.tar.gz"
url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

note "installing ${BIN} ${tag} (${target}) -> ${install_dir}"

# --- download + extract ---
tmp=$(mktemp -d 2>/dev/null || mktemp -d -t ccs)
trap 'rm -rf "$tmp"' EXIT INT TERM

curl -fSL "$url" -o "$tmp/$asset" || err "download failed: $url"
tar -xzf "$tmp/$asset" -C "$tmp"

# Asset contains a renamed binary like cc-session-<target>; normalize.
src="$tmp/${BIN}-${target}"
if [ ! -f "$src" ]; then
  # Fallback: pick any cc-session-* file in tmp.
  src=$(find "$tmp" -maxdepth 2 -type f -name "${BIN}*" | head -n 1)
  [ -n "$src" ] || err "extracted archive missing ${BIN} binary"
fi
chmod +x "$src"
mv "$src" "$install_dir/$BIN"

# --- PATH hint ---
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *)
    note ""
    note "warning: $install_dir is not on your PATH."
    note "add this to your shell rc:"
    note "    export PATH=\"$install_dir:\$PATH\""
    ;;
esac

note ""
note "installed: $($install_dir/$BIN --version 2>/dev/null || echo "$install_dir/$BIN")"
note "run 'cc-session --help' to get started."
