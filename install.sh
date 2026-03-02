#!/usr/bin/env sh
set -eu

REPO="KaspaNameService/kns-inscribe"
BIN_NAME="kns-inscribe"

VERSION="${KNS_INSCRIBE_VERSION:-latest}"
BIN_DIR="${KNS_INSCRIBE_BIN_DIR:-"$HOME/.local/bin"}"

usage() {
  cat <<'EOF'
Install kns-inscribe (prebuilt GitHub Release binary)

Usage:
  install.sh [--version <tag>|latest] [--bin-dir <dir>]

Examples:
  curl -fsSL https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.sh | sh
  curl -fsSL https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.sh | sh -s -- --bin-dir "$HOME/bin"
  KNS_INSCRIBE_VERSION=v1.2.3 curl -fsSL https://github.com/KaspaNameService/kns-inscribe/releases/latest/download/install.sh | sh

Env vars:
  KNS_INSCRIBE_VERSION   Release tag (e.g. v1.2.3) or 'latest'
  KNS_INSCRIBE_BIN_DIR   Install directory (default: ~/.local/bin)
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -v|--version)
      VERSION="${2:-}"
      shift 2
      ;;
    -b|--bin-dir)
      BIN_DIR="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$VERSION" ] || [ -z "$BIN_DIR" ]; then
  echo "Missing --version or --bin-dir value" >&2
  usage >&2
  exit 2
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux) OS_ID="linux" ;;
  Darwin) OS_ID="macos" ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH_ID="x86_64" ;;
  arm64|aarch64) ARCH_ID="arm64" ;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

ASSET_SUFFIX="$OS_ID-$ARCH_ID"
ASSET_NAME="$BIN_NAME-$ASSET_SUFFIX"

if [ "$VERSION" = "latest" ]; then
  BASE_URL="https://github.com/$REPO/releases/latest/download"
else
  BASE_URL="https://github.com/$REPO/releases/download/$VERSION"
fi

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t kns-inscribe)"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT INT HUP TERM

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

echo "Downloading $BIN_NAME ($ASSET_SUFFIX) from $REPO ($VERSION)..." >&2

BIN_PATH="$TMP_DIR/$ASSET_NAME"
SHA_PATH="$TMP_DIR/$ASSET_NAME.sha256"

curl -fL "$BASE_URL/$ASSET_NAME" -o "$BIN_PATH"
curl -fL "$BASE_URL/$ASSET_NAME.sha256" -o "$SHA_PATH"

verify_sha() {
  expected="$(cut -d ' ' -f 1 "$SHA_PATH" | tr -d '\n' | tr -d '\r')"
  if [ -z "$expected" ]; then
    echo "Checksum file format unexpected: $SHA_PATH" >&2
    return 1
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$BIN_PATH" | cut -d ' ' -f 1)"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$BIN_PATH" | cut -d ' ' -f 1)"
  else
    echo "No sha256 tool found (sha256sum/shasum); skipping checksum verification" >&2
    return 0
  fi

  if [ "$expected" != "$actual" ]; then
    echo "Checksum mismatch for $ASSET_NAME" >&2
    echo "Expected: $expected" >&2
    echo "Actual:   $actual" >&2
    return 1
  fi
}

verify_sha

mkdir -p "$BIN_DIR"
cp "$BIN_PATH" "$BIN_DIR/$BIN_NAME"
chmod 0755 "$BIN_DIR/$BIN_NAME"

echo "Installed: $BIN_DIR/$BIN_NAME" >&2

case ":$PATH:" in
  *":$BIN_DIR:"*)
    ;;
  *)
    echo "NOTE: $BIN_DIR is not on PATH." >&2
    echo "Add this to your shell profile:" >&2
    echo "  export PATH=\"$BIN_DIR:\$PATH\"" >&2
    ;;
esac

echo "Verify: $BIN_NAME --version" >&2
