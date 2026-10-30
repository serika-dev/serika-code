#!/usr/bin/env bash
# Serika Code installer for Linux & macOS
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/serika-dev/serika-code/main/install.sh | bash
#   ./install.sh [--from-source] [--prefix ~/.local] [--version v0.1.0]
set -euo pipefail

REPO="${SERIKA_REPO:-serika-dev/serika-code}"
PREFIX="${SERIKA_PREFIX:-$HOME/.local}"
VERSION="${SERIKA_VERSION:-latest}"
FROM_SOURCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from-source)
      FROM_SOURCE=1
      shift
      ;;
    --prefix)
      PREFIX="$2"
      shift 2
      ;;
    --version)
      VERSION="$2"
      shift 2
      ;;
    -h|--help)
      echo "Serika Code installer"
      echo "  --from-source       Compile from source with cargo"
      echo "  --prefix <dir>      Install prefix (default: ~/.local)"
      echo "  --version <tag>     Release tag (default: latest)"
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64|amd64) TARGET="x86_64-unknown-linux-gnu" ;;
      *) echo "Unsupported Linux architecture: $ARCH (use --from-source)" >&2; FROM_SOURCE=1 ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      arm64|aarch64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS (on Windows, run: powershell -ExecutionPolicy Bypass -File install.ps1)" >&2
    exit 1
    ;;
esac

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

install_from_dir() {
  local src_dir="$1"
  mkdir -p "$PREFIX/bin"
  install -m 0755 "$src_dir/serika" "$PREFIX/bin/serika"
  if [[ -f "$src_dir/serika-desktop" ]]; then
    install -m 0755 "$src_dir/serika-desktop" "$PREFIX/bin/serika-desktop"
  fi
  "$PREFIX/bin/serika" setup --prefix "$PREFIX"
}

if [[ "$FROM_SOURCE" -eq 0 && -f "./target/release/serika" ]]; then
  echo "==> Installing from local ./target/release build..."
  install_from_dir "./target/release"
  exit 0
fi

if [[ "$FROM_SOURCE" -eq 0 ]]; then
  ASSET="serika-code-${TARGET}.tar.gz"
  if [[ "$VERSION" == "latest" ]]; then
    BASE_URL="https://github.com/${REPO}/releases/latest/download"
  else
    BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
  fi

  AUTH_ARGS=()
  if [[ -n "${SERIKA_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
    AUTH_ARGS=(-H "Authorization: Bearer ${SERIKA_GITHUB_TOKEN:-${GITHUB_TOKEN}}")
  fi

  echo "==> Downloading ${ASSET} from ${REPO} (${VERSION})..."
  if curl -fsSL "${AUTH_ARGS[@]}" "${BASE_URL}/${ASSET}" -o "${TMP_DIR}/${ASSET}" 2>/dev/null; then
    if curl -fsSL "${AUTH_ARGS[@]}" "${BASE_URL}/SHA256SUMS" -o "${TMP_DIR}/SHA256SUMS" 2>/dev/null; then
      EXPECTED="$(grep "${ASSET}" "${TMP_DIR}/SHA256SUMS" | awk '{print $1}' | head -n1)"
      if [[ -n "$EXPECTED" ]]; then
        if command -v sha256sum >/dev/null 2>&1; then
          ACTUAL="$(sha256sum "${TMP_DIR}/${ASSET}" | awk '{print $1}')"
        else
          ACTUAL="$(shasum -a 256 "${TMP_DIR}/${ASSET}" | awk '{print $1}')"
        fi
        if [[ "$ACTUAL" != "$EXPECTED" ]]; then
          echo "SHA-256 verification failed for ${ASSET}!" >&2
          exit 1
        fi
        echo "==> SHA-256 checksum verified."
      fi
    fi
    mkdir -p "${TMP_DIR}/extracted"
    tar -xzf "${TMP_DIR}/${ASSET}" -C "${TMP_DIR}/extracted"
    BIN_FOUND="$(find "${TMP_DIR}/extracted" -name serika -type f | head -n1)"
    if [[ -n "$BIN_FOUND" ]]; then
      install_from_dir "$(dirname "$BIN_FOUND")"
      exit 0
    fi
  else
    echo "==> No prebuilt release archive found at ${BASE_URL}/${ASSET}; falling back to source build..."
  fi
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to build from source. Install Rust via https://rustup.rs" >&2
  exit 1
fi

if [[ -f "./Cargo.toml" && -d "./crates/serika-cli" ]]; then
  echo "==> Building Serika Code from current repository..."
  cargo build --release --workspace
  install_from_dir "./target/release"
else
  echo "==> Cloning https://github.com/${REPO}.git..."
  git clone --depth 1 "https://github.com/${REPO}.git" "${TMP_DIR}/src"
  (cd "${TMP_DIR}/src" && cargo build --release --workspace)
  install_from_dir "${TMP_DIR}/src/target/release"
fi
