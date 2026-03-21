#!/usr/bin/env bash
set -euo pipefail

REPO="tatolab/amos"
INSTALL_DIR="${AMOS_INSTALL_DIR:-$HOME/.local/bin}"
SKILLS_DIR="$HOME/.claude/skills"

# Colors (disabled if not a terminal)
if [ -t 1 ]; then
  BOLD='\033[1m' DIM='\033[2m' GREEN='\033[32m' RED='\033[31m' RESET='\033[0m'
else
  BOLD='' DIM='' GREEN='' RED='' RESET=''
fi

info()  { echo -e "${BOLD}amos:${RESET} $*"; }
ok()    { echo -e "${GREEN}✓${RESET} $*"; }
fail()  { echo -e "${RED}✗${RESET} $*" >&2; exit 1; }

# Detect platform and architecture
detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux*)  os="linux" ;;
    Darwin*) os="darwin" ;;
    MINGW*|MSYS*|CYGWIN*) os="windows" ;;
    *) fail "unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64)  arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *) fail "unsupported architecture: $arch" ;;
  esac

  echo "${os}-${arch}"
}

# Map platform to artifact name from CI
artifact_name() {
  local platform="$1"
  case "$platform" in
    linux-x86_64)   echo "amos-linux-x86_64" ;;
    darwin-x86_64)  echo "amos-darwin-x86_64" ;;
    darwin-aarch64) echo "amos-darwin-aarch64" ;;
    windows-x86_64) echo "amos-windows-x86_64.exe" ;;
    *) fail "no binary available for $platform" ;;
  esac
}

# Find the latest release tag, or fall back to building from source
download_or_build() {
  local platform="$1"
  local artifact
  artifact="$(artifact_name "$platform")"

  # Try downloading from latest GitHub release
  local release_url="https://github.com/${REPO}/releases/latest/download/${artifact}"

  info "downloading ${DIM}${release_url}${RESET}"
  if curl -fsSL -o "${INSTALL_DIR}/amos" "$release_url" 2>/dev/null; then
    chmod +x "${INSTALL_DIR}/amos"
    ok "downloaded amos binary"
    return 0
  fi

  # No release available — build from source
  info "no release found, building from source..."
  if ! command -v cargo &>/dev/null; then
    fail "cargo not found — install Rust first: https://rustup.rs"
  fi

  local tmpdir
  tmpdir="$(mktemp -d)"
  trap "rm -rf '$tmpdir'" EXIT

  git clone --depth 1 "https://github.com/${REPO}.git" "$tmpdir/amos" 2>/dev/null
  cargo build --release --manifest-path "$tmpdir/amos/Cargo.toml"
  cp "$tmpdir/amos/target/release/amos" "${INSTALL_DIR}/amos"
  chmod +x "${INSTALL_DIR}/amos"
  ok "built amos from source"
}

# Install Claude Code skills from the repo
install_skills() {
  local skills_src="https://raw.githubusercontent.com/${REPO}/main/.claude/skills"
  local skills=("amos" "amos-graph" "amos-show" "amos-sync" "amos-notify" "amos-create" "amos-prune")

  info "installing Claude Code skills to ${DIM}${SKILLS_DIR}${RESET}"

  for skill in "${skills[@]}"; do
    local dir="${SKILLS_DIR}/${skill}"
    mkdir -p "$dir"
    if curl -fsSL -o "$dir/SKILL.md" "${skills_src}/${skill}/SKILL.md" 2>/dev/null; then
      ok "  ${skill}"
    else
      echo -e "  ${DIM}skipped ${skill} (not found)${RESET}"
    fi
  done
}

# Ensure install dir is on PATH
check_path() {
  case ":$PATH:" in
    *":${INSTALL_DIR}:"*) return 0 ;;
  esac

  info "${INSTALL_DIR} is not on your PATH"
  echo ""
  echo "  Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
  echo ""
  echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
  echo ""
}

main() {
  local platform
  platform="$(detect_platform)"
  info "detected platform: ${BOLD}${platform}${RESET}"

  # Create install directory
  mkdir -p "$INSTALL_DIR"

  # Download binary or build from source
  download_or_build "$platform"

  # Install Claude Code skills
  install_skills

  # Verify
  if "${INSTALL_DIR}/amos" --version &>/dev/null; then
    echo ""
    ok "amos $(${INSTALL_DIR}/amos --version 2>&1 | head -1) installed to ${INSTALL_DIR}/amos"
  else
    ok "amos installed to ${INSTALL_DIR}/amos"
  fi

  # Check PATH
  check_path

  echo ""
  info "run ${BOLD}amos --help${RESET} to get started"
}

main "$@"
