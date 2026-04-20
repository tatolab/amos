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

# Install Claude Code skills from the repo.
#
# Discovers the skill list dynamically from the GitHub contents API instead
# of hardcoding it — that way new skills added to the repo are picked up
# automatically on the next install. Also copies every file inside each skill
# directory (SKILL.md plus any `references/` subdirectory), fixing a previous
# bug where only SKILL.md was fetched and skills referencing `references/`
# files shipped broken.
install_skills() {
  local api_base="https://api.github.com/repos/${REPO}/contents/.claude/skills"
  local raw_base="https://raw.githubusercontent.com/${REPO}/main/.claude/skills"

  info "installing Claude Code skills to ${DIM}${SKILLS_DIR}${RESET}"

  local skills_json
  skills_json="$(curl -fsSL -H "Accept: application/vnd.github+json" "$api_base" 2>/dev/null)" || {
    echo -e "  ${RED}✗${RESET} failed to list skills from ${api_base}"
    echo -e "  ${DIM}(are we rate-limited? try again in a minute)${RESET}"
    return 1
  }

  # Extract skill directory names. Works without jq by parsing the JSON
  # minimally — GitHub's contents API returns `{"name": "...", "type": "dir", ...}` entries.
  local skills=()
  while IFS= read -r skill; do
    skills+=("$skill")
  done < <(
    printf '%s\n' "$skills_json" \
      | grep -oE '"name": *"[^"]+"' \
      | head -n 200 \
      | awk -F'"' '{ print $4 }'
  )

  if [ ${#skills[@]} -eq 0 ]; then
    echo -e "  ${DIM}no skills found at ${api_base}${RESET}"
    return 0
  fi

  for skill in "${skills[@]}"; do
    local dir="${SKILLS_DIR}/${skill}"
    mkdir -p "$dir"

    # Fetch the skill's file listing recursively and download each blob.
    local tree_json
    tree_json="$(curl -fsSL -H "Accept: application/vnd.github+json" \
      "${api_base}/${skill}?recursive=1" 2>/dev/null)" || {
      echo -e "  ${DIM}skipped ${skill} (listing failed)${RESET}"
      continue
    }

    # For each file entry, reconstruct its relative path and download.
    local got_files=0
    while IFS= read -r relpath; do
      [ -z "$relpath" ] && continue
      local target="${dir}/${relpath}"
      mkdir -p "$(dirname "$target")"
      if curl -fsSL -o "$target" "${raw_base}/${skill}/${relpath}" 2>/dev/null; then
        got_files=$((got_files + 1))
      fi
    done < <(
      printf '%s\n' "$tree_json" \
        | grep -oE '"path": *"[^"]+"' \
        | awk -F'"' '{ print $4 }'
    )

    if [ $got_files -gt 0 ]; then
      ok "  ${skill} (${got_files} file$([ $got_files -ne 1 ] && echo s))"
    else
      # Fall back to just SKILL.md so a partial install still has something useful.
      if curl -fsSL -o "${dir}/SKILL.md" "${raw_base}/${skill}/SKILL.md" 2>/dev/null; then
        ok "  ${skill} (SKILL.md only)"
      else
        echo -e "  ${DIM}skipped ${skill} (not found)${RESET}"
      fi
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
