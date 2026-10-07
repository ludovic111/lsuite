#!/usr/bin/env bash
# Builds the launcher for x86_64 Linux and packages it (run on Linux, e.g. Ubuntu 22.04 for an
# old enough glibc):
#   scripts/bundle-linux.sh
# Writes to target/dist/:
#   lsuite-linux-x86_64.AppImage   one-file app (updates itself)
#   lsuite-linux-x86_64.tar.gz     bin/ and lib/ to unpack anywhere (updated by hand)
# These are the names lsuite.xyz/launcher/download/linux-* look for.
#
# The binaries look for shared libraries in ../lib first (rpath), where the libraries ldd finds
# are copied, except glibc's and the GPU/display stack the system must provide.
#
# Environment (all optional):
#   APPIMAGETOOL        path to appimagetool (downloaded otherwise)
#   LSUITE_SKIP_BUILD=1 reuse the binaries already in target/<triple>/release
#   CARGO               the cargo to run (default: cargo)
set -euo pipefail
cd "$(dirname "$0")/.."

triple=x86_64-unknown-linux-gnu
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$version" ] || { echo "No version in Cargo.toml [workspace.package]" >&2; exit 1; }
cargo=${CARGO:-cargo}
resources=crates/lsuite-desktop/resources
dist=target/dist
work="$dist/$triple"
bins=(lsuite lsuite-cli lsuite-mcp)

if [ "${LSUITE_SKIP_BUILD:-}" != 1 ]; then
  locked=()
  [ -n "${CI:-}" ] && locked=(--locked)
  packages=()
  for b in "${bins[@]}"; do packages+=(-p "$b"); done
  RUSTFLAGS="${RUSTFLAGS:-} -C link-args=-Wl,--disable-new-dtags,-rpath,\$ORIGIN/../lib" \
    "$cargo" build --release ${locked[@]+"${locked[@]}"} --target "$triple" "${packages[@]}"
fi

# One tree for both packages: bin/ (our binaries), lib/ (bundled libraries), share/.
tree="$work/lsuite"
rm -rf "$work"
mkdir -p "$tree/bin" "$tree/lib" "$tree/share/applications" "$tree/share/icons/hicolor/512x512/apps" "$tree/share/doc/lsuite"
for b in "${bins[@]}"; do
  cp "target/$triple/release/$b" "$tree/bin/$b"
  strip --strip-debug "$tree/bin/$b" || true
done
cp LICENSE "$tree/share/doc/lsuite/"
cp "$resources/lsuite.desktop" "$tree/share/applications/xyz.lsuite.launcher.desktop"
cp "$resources/lsuite.png" "$tree/share/icons/hicolor/512x512/apps/lsuite.png"

skip='^(linux-vdso|ld-linux|libc|libm|libdl|libpthread|librt|libresolv|libgcc_s|libstdc\+\+|libGL|libEGL|libGLX|libGLdispatch|libvulkan|libdrm|libgbm|libwayland|libX|libxcb|libxkbcommon|libasound|libdbus-1|libsystemd|libfontconfig|libfreetype|libexpat|libz)\.'
ldd "$tree/bin/lsuite" | awk '/=> \// { print $1 " " $3 }' | while read -r name path; do
  if ! printf '%s\n' "$name" | grep -Eq "$skip"; then
    cp -L "$path" "$tree/lib/"
    echo "bundled $name"
  fi
done

# ---- AppImage -------------------------------------------------------------
appdir="$work/lsuite.AppDir"
mkdir -p "$appdir/usr"
cp -a "$tree/bin" "$tree/lib" "$tree/share" "$appdir/usr/"
cp "$resources/lsuite.desktop" "$appdir/lsuite.desktop"
cp "$resources/lsuite.png" "$appdir/lsuite.png"
ln -s lsuite.png "$appdir/.DirIcon"
cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/lsuite" "$@"
APPRUN
chmod 755 "$appdir/AppRun"

tool=${APPIMAGETOOL:-}
if [ -z "$tool" ]; then
  tool="$work/appimagetool"
  curl -fsSL --retry 3 -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod 755 "$tool"
fi
appimage="$dist/lsuite-linux-x86_64.AppImage"
rm -f "$appimage"
# No FUSE on CI runners: let the tool unpack itself.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 VERSION="$version" "$tool" --no-appstream "$appdir" "$appimage"
chmod 755 "$appimage"

# ---- tar.gz ---------------------------------------------------------------
tarball="$dist/lsuite-linux-x86_64.tar.gz"
rm -f "$tarball"
tar -C "$work" -czf "$tarball" lsuite

echo "Built lsuite $version for $triple:"
ls -lh "$appimage" "$tarball"
