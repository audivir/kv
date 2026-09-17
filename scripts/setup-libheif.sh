#!/usr/bin/env bash
# Downloads the prebuilt static libheif (audivir/libheif-static) for the current OS/architecture
# into vendor/libheif-static, where .cargo/config.toml points PKG_CONFIG_PATH.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
vendor_dir="$script_dir/../vendor/libheif-static"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset="macos-arm64" ;;
  Linux-x86_64) asset="linux-amd64" ;;
  Linux-aarch64) asset="linux-arm64" ;;
  MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64) asset="windows-amd64" ;;
  MINGW*-aarch64 | MSYS*-aarch64 | CYGWIN*-aarch64) asset="windows-arm64" ;;
  *)
    echo "No prebuilt static libheif available for $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

if [ -f "$vendor_dir/lib/pkgconfig/libheif.pc" ]; then
  echo "Static libheif already present at $vendor_dir" >&2
  exit 0
fi

url="https://github.com/audivir/libheif-static/releases/latest/download/libheif-static-$asset.tar.gz"
mkdir -p "$vendor_dir"
echo "Downloading static libheif ($asset)..." >&2
curl --fail-with-body -sL "$url" | tar -xz -C "$vendor_dir"
