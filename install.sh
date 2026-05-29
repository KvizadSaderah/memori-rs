#!/usr/bin/env bash
# memori-rs installer — downloads a prebuilt binary from GitHub Releases,
# installs it to a bin directory on your PATH, and wires up your AI clients.
#
#   curl -fsSL https://raw.githubusercontent.com/KvizadSaderah/memori-rs/main/install.sh | bash
#
# Environment overrides:
#   MEMORI_INSTALL_DIR   install location (default: ~/.local/bin)
#   MEMORI_VERSION       release tag to install (default: latest)
#   MEMORI_NO_INIT=1     skip `memori init` / `memori doctor` after install
set -euo pipefail

REPO="KvizadSaderah/memori-rs"
INSTALL_DIR="${MEMORI_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${MEMORI_VERSION:-latest}"

err()  { printf '\033[31merror:\033[0m %s\n' "$1" >&2; exit 1; }
info() { printf '\033[36m==>\033[0m %s\n' "$1"; }

# --- detect platform -------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin)
    case "$arch" in
      arm64|aarch64) target="aarch64-apple-darwin" ;;
      x86_64)        target="x86_64-apple-darwin" ;;
      *) err "unsupported macOS arch: $arch" ;;
    esac ;;
  Linux)
    case "$arch" in
      x86_64) target="x86_64-unknown-linux-gnu" ;;
      *) err "unsupported Linux arch: $arch (build from source: cargo install --git https://github.com/$REPO memori-rs)" ;;
    esac ;;
  *) err "unsupported OS: $os — on Windows use the .zip from the Releases page" ;;
esac

asset="memori-${target}.tar.gz"

# --- resolve download URL --------------------------------------------------
if [ "$VERSION" = "latest" ]; then
  url="https://github.com/$REPO/releases/latest/download/$asset"
else
  url="https://github.com/$REPO/releases/download/$VERSION/$asset"
fi

# --- download & extract ----------------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

info "downloading $asset ($VERSION)"
if ! curl -fsSL "$url" -o "$tmp/$asset"; then
  err "download failed: $url
The release may not exist yet. Build from source instead:
  cargo install --git https://github.com/$REPO memori-rs"
fi

# verify checksum if published alongside the asset
if curl -fsSL "$url.sha256" -o "$tmp/$asset.sha256" 2>/dev/null; then
  info "verifying checksum"
  expected="$(awk '{print $1}' "$tmp/$asset.sha256")"
  if command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp/$asset" | awk '{print $1}')"
  else
    actual="$(sha256sum "$tmp/$asset" | awk '{print $1}')"
  fi
  [ "$expected" = "$actual" ] || err "checksum mismatch (expected $expected, got $actual)"
fi

tar -xzf "$tmp/$asset" -C "$tmp"

# --- install ---------------------------------------------------------------
mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp/memori" "$INSTALL_DIR/memori"
info "installed memori → $INSTALL_DIR/memori"

# warn if not on PATH
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) printf '\033[33mnote:\033[0m %s is not on your PATH. Add to your shell profile:\n  export PATH="%s:$PATH"\n' "$INSTALL_DIR" "$INSTALL_DIR" ;;
esac

# --- wire up + verify ------------------------------------------------------
if [ "${MEMORI_NO_INIT:-0}" != "1" ]; then
  info "wiring up AI clients (memori init)"
  "$INSTALL_DIR/memori" init || true
  info "verifying (memori doctor)"
  "$INSTALL_DIR/memori" doctor || true
fi

info "done. Restart your AI client; in Claude Code run /mcp to confirm memori is listed."
