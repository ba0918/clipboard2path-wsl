#!/usr/bin/env bash
set -euo pipefail

# clipboard2path-wsl installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ba0918/clipboard2path-wsl/main/scripts/install.sh | bash

REPO="ba0918/clipboard2path-wsl"
BINARY_NAME="clipboard2path-wsl"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# ── Colors ──────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[info]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC}  $*"; }
die()   { echo -e "${RED}[error]${NC} $*" >&2; exit 1; }

# ── Pre-flight checks ──────────────────────────────────
if ! grep -qi microsoft /proc/version 2>/dev/null; then
  warn "This tool is designed for WSL2. Proceeding anyway..."
fi

command -v curl >/dev/null 2>&1 || die "curl is required."
command -v tar  >/dev/null 2>&1 || die "tar is required."

# ── Detect architecture ─────────────────────────────────
arch=$(uname -m)
case "$arch" in
  x86_64)  asset_arch="x86_64" ;;
  *)       die "Unsupported architecture: $arch (only x86_64 is supported)" ;;
esac

# ── Fetch latest release ────────────────────────────────
info "Fetching latest release from ${REPO}..."

api_url="https://api.github.com/repos/${REPO}/releases/latest"
release_json=$(curl -fsSL "$api_url") || die "Failed to fetch release info. Is the repository public?"

tag=$(echo "$release_json" | grep '"tag_name"' | head -1 | sed 's/.*: *"\(.*\)".*/\1/')
if [ -z "$tag" ]; then
  die "Could not determine latest release tag."
fi

info "Latest release: ${tag}"

# ── Find asset URL ──────────────────────────────────────
asset_pattern="${BINARY_NAME}-${tag}-${asset_arch}-linux.tar.gz"
download_url=$(echo "$release_json" | grep '"browser_download_url"' | grep "$asset_pattern" | head -1 | sed 's/.*: *"\(.*\)".*/\1/')

if [ -z "$download_url" ]; then
  die "Could not find asset: ${asset_pattern}"
fi

# ── Download and extract ────────────────────────────────
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

info "Downloading ${asset_pattern}..."
curl -fsSL -o "${tmpdir}/archive.tar.gz" "$download_url" || die "Download failed."

info "Extracting..."
tar xzf "${tmpdir}/archive.tar.gz" -C "$tmpdir" || die "Extraction failed."

if [ ! -f "${tmpdir}/${BINARY_NAME}" ]; then
  die "Binary not found in archive."
fi

# ── Install ─────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
mv "${tmpdir}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
ok "Installed to ${INSTALL_DIR}/${BINARY_NAME}"

# ── Verify ──────────────────────────────────────────────
if command -v "$BINARY_NAME" >/dev/null 2>&1; then
  ok "Installation complete! (${tag})"
else
  warn "Installed, but ${INSTALL_DIR} is not in your PATH."
  echo ""
  echo "Add it to your shell config:"
  echo ""
  echo "  # fish"
  echo "  fish_add_path ${INSTALL_DIR}"
  echo ""
  echo "  # bash/zsh"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
fi

echo ""
info "Next steps:"
echo "  ${BINARY_NAME} init    # Install shell hooks + systemd service"
echo "  ${BINARY_NAME} status  # Check setup status"
