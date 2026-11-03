#!/usr/bin/env bash
# Package the Qt desktop for Linux: an AppImage (any distribution) and a .deb (Debian, Ubuntu
# and friends). Both carry their own Qt and the serika command line.
#
#   packaging/linux/build.sh                    # builds the CLI with cargo first
#   packaging/linux/build.sh path/to/serika     # bundles a CLI built elsewhere (CI)
#
# Needs Qt 6 (qmake on PATH, or QMAKE=/path/to/qmake), cmake, ninja, curl and dpkg-deb.
# Build on the oldest distribution you want to support (CI uses Ubuntu 22.04): the packages
# run on that glibc and anything newer.
#
# Writes into dist/:
#   serika-code-<version>-x86_64.AppImage (+ .zsync, so AppImageUpdate can fetch only changes)
#   serika-code_<version>_amd64.deb
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
DIST="$ROOT/dist"
APPDIR="$DIST/AppDir"
BUILD="$ROOT/build/linux-qt"
TOOLS="$ROOT/build/linux-tools"

CLI=${1:-}
if [ -z "$CLI" ]; then
  cargo build --release -p serika-cli
  CLI="$ROOT/target/release/serika"
fi
QMAKE=${QMAKE:-$(command -v qmake6 || command -v qmake)}
export QMAKE
QT_PREFIX=$("$QMAKE" -query QT_INSTALL_PREFIX)
echo "Serika Code $VERSION, Qt from $QT_PREFIX"

# 1. The app
cmake -S qt/serika-code -B "$BUILD" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="$QT_PREFIX"
cmake --build "$BUILD"

# 2. The AppDir: the usual /usr layout, which both packages start from
rm -rf "$APPDIR"
install -Dm755 "$BUILD/serika-code" "$APPDIR/usr/bin/serika-code"
install -Dm755 "$CLI" "$APPDIR/usr/bin/serika"
install -Dm644 packaging/linux/serika-code.desktop "$APPDIR/usr/share/applications/serika-code.desktop"
install -Dm644 packaging/linux/dev.serika.SerikaCode.metainfo.xml \
  "$APPDIR/usr/share/metainfo/dev.serika.SerikaCode.metainfo.xml"
for size in 16 32 48 64 128 256 512; do
  install -Dm644 "qt/serika-code/app/serika-code-$size.png" \
    "$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps/serika-code.png"
done

# 3. Qt and the other libraries the binaries need, next to them
mkdir -p "$TOOLS"
fetch() {
  [ -x "$TOOLS/$1" ] || { curl -fsSL -o "$TOOLS/$1" "$2"; chmod +x "$TOOLS/$1"; }
}
fetch linuxdeploy-x86_64.AppImage \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
fetch linuxdeploy-plugin-qt-x86_64.AppImage \
  https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage
export PATH="$TOOLS:$PATH"
export APPIMAGE_EXTRACT_AND_RUN=1               # no FUSE needed on build machines
export QML_SOURCES_PATHS="$ROOT/qt/serika-code/qml"
export EXTRA_QT_MODULES="svg"                   # the icons are SVG
export EXTRA_PLATFORM_PLUGINS="libqwayland-egl.so;libqwayland-generic.so"
export LDAI_UPDATE_INFORMATION="gh-releases-zsync|serika-dev|serika-code|latest|serika-code-*-x86_64.AppImage.zsync"
export LDAI_OUTPUT="serika-code-$VERSION-x86_64.AppImage"

cd "$DIST"
rm -f serika-code-*-x86_64.AppImage serika-code-*-x86_64.AppImage.zsync
linuxdeploy-x86_64.AppImage --appdir "$APPDIR" \
  --executable "$APPDIR/usr/bin/serika-code" \
  --executable "$APPDIR/usr/bin/serika" \
  --desktop-file "$APPDIR/usr/share/applications/serika-code.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/serika-code.png" \
  --plugin qt \
  --output appimage
cd "$ROOT"

# 4. The .deb: the same files under /opt/serika-code, with commands in /usr/bin and the
#    launcher, icons and metadata where the desktop looks for them.
PKG="$DIST/deb/serika-code_${VERSION}_amd64"
rm -rf "$DIST/deb"
mkdir -p "$PKG/DEBIAN" "$PKG/opt/serika-code" "$PKG/usr/bin" "$PKG/usr/share"
cp -a "$APPDIR/usr/." "$PKG/opt/serika-code/"
for dir in applications icons metainfo; do
  mv "$PKG/opt/serika-code/share/$dir" "$PKG/usr/share/$dir"
done
rmdir "$PKG/opt/serika-code/share" 2>/dev/null || true
ln -s /opt/serika-code/bin/serika-code "$PKG/usr/bin/serika-code"
ln -s /opt/serika-code/bin/serika "$PKG/usr/bin/serika"
install -Dm644 LICENSE "$PKG/usr/share/doc/serika-code/copyright"

cat > "$PKG/DEBIAN/control" <<EOF
Package: serika-code
Version: $VERSION
Section: devel
Priority: optional
Architecture: amd64
Maintainer: Serika <58044069+Pikachubolk@users.noreply.github.com>
Homepage: https://github.com/serika-dev/serika-code
Installed-Size: $(du -sk "$PKG" | cut -f1)
Depends: libc6 (>= 2.35), libstdc++6, libgcc-s1, zlib1g, libglib2.0-0, libdbus-1-3, libfontconfig1, libfreetype6, libgl1, libegl1, libx11-6, libx11-xcb1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libxcb-cursor0 | libxcb-cursor-dev
Recommends: git
Description: Fast, native coding agent for your desktop and terminal
 Chat with any model, let it read and edit your project, run commands, browse
 the web, and review every change, in one native app. Includes the desktop app
 (serika-code) and the command line (serika).
EOF

dpkg-deb --build --root-owner-group -Zxz "$PKG" "$DIST/serika-code_${VERSION}_amd64.deb"
rm -rf "$DIST/deb"

cd "$DIST"
for file in "serika-code-$VERSION-x86_64.AppImage" "serika-code_${VERSION}_amd64.deb"; do
  sha256sum "$file" > "$file.sha256"
done
ls -lh serika-code*
