#!/usr/bin/env bash
# Usage: aur/bump.sh <version> [aur-dir]
# Sets pkgver/pkgrel in every <aur-dir>/*/PKGBUILD, updates the checksums and regenerates .SRCINFO.
# Needs makepkg and updpkgsums (pacman-contrib), and network access to download the sources.
set -euo pipefail

version="${1:-}"
dir="${2:-$(dirname "${BASH_SOURCE[0]}")}"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 <version like 1.2.3> [aur-dir]" >&2
  exit 1
fi

shopt -s nullglob
pkgbuilds=("$dir"/*/PKGBUILD)
if [ ${#pkgbuilds[@]} -eq 0 ]; then
  echo "no PKGBUILD found in $dir/*/" >&2
  exit 1
fi

for pkgbuild in "${pkgbuilds[@]}"; do
  pkgdir=$(dirname "$pkgbuild")
  sed -i -e "s/^pkgver=.*/pkgver=$version/" -e 's/^pkgrel=.*/pkgrel=1/' "$pkgbuild"
  (
    cd "$pkgdir"
    updpkgsums
    makepkg --printsrcinfo > .SRCINFO
  )
  echo "bumped $(basename "$pkgdir") to $version"
done
