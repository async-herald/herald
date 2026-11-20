#!/usr/bin/env bash
# Paquets Linux d'Async Herald à partir du binaire déjà compilé
# (`cargo build --release --bin herald-desktop`) :
#   dist/Async-Herald-<version>-<arch>.AppImage  (toutes distributions)
#   dist/async-herald_<version>_<arch>.deb       (Debian, Ubuntu)
# puis affiche leur empreinte SHA-256.
#
# Prérequis : dpkg-dev (dpkg-deb, dpkg-shlibdeps), curl, file, desktop-file-utils.
# Utilisé par .github/workflows/linux.yml ; marche aussi dans un conteneur Ubuntu.
set -euo pipefail

cd "$(dirname "$0")/../.."
ROOT="$PWD"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
# Binaire du workspace Cargo : `target/` à la racine du dépôt.
BIN="${HERALD_BIN:-../../target/release/herald-desktop}"
case "$BIN" in /*) ;; *) BIN="$ROOT/$BIN" ;; esac
LOGO="$ROOT/assets/brand/angel.png"
DESKTOP="$ROOT/scripts/linux/async-herald.desktop"
APPIMAGETOOL_VERSION="1.9.0"

case "$(uname -m)" in
  x86_64) ARCH=x86_64 DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64 DEB_ARCH=arm64 ;;
  *) echo "architecture non prise en charge : $(uname -m)" >&2; exit 1 ;;
esac

[ -x "$BIN" ] || { echo "binaire introuvable : $BIN (lancer cargo build --release)" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p dist

# --- .deb -------------------------------------------------------------------
PKG="$WORK/deb"
install -Dm755 "$BIN" "$PKG/usr/bin/herald-desktop"
install -Dm644 "$DESKTOP" "$PKG/usr/share/applications/async-herald.desktop"
install -Dm644 "$LOGO" "$PKG/usr/share/icons/hicolor/256x256/apps/async-herald.png"

# Dépendances déduites des bibliothèques réellement liées au binaire.
mkdir -p "$WORK/shlib/debian"
printf 'Source: async-herald\n\nPackage: async-herald\nArchitecture: any\n' > "$WORK/shlib/debian/control"
DEPENDS="$(cd "$WORK/shlib" && dpkg-shlibdeps -O "$BIN" | sed -n 's/^shlibs:Depends=//p')"
[ -n "$DEPENDS" ] || { echo "dépendances introuvables (dpkg-shlibdeps)" >&2; exit 1; }
# Chargées au démarrage par GPUI (dlopen), donc invisibles pour dpkg-shlibdeps.
DEPENDS="$DEPENDS, libvulkan1, libwayland-client0"
RECOMMENDS="mesa-vulkan-drivers"

mkdir -p "$PKG/DEBIAN"
cat > "$PKG/DEBIAN/control" <<CONTROL
Package: async-herald
Version: $VERSION
Section: devel
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Its Async <contact@herald.codes>
Homepage: https://herald.codes
Depends: $DEPENDS
Recommends: $RECOMMENDS
Installed-Size: $(du -sk "$PKG/usr" | cut -f1)
Description: Async Herald, analyse de code locale
 Analyse la qualité et la sécurité d'un projet sur votre machine,
 sans envoyer le code.
CONTROL
DEB="dist/async-herald_${VERSION}_${DEB_ARCH}.deb"
dpkg-deb --build --root-owner-group "$PKG" "$DEB" >/dev/null

# --- AppImage ---------------------------------------------------------------
APPDIR="$WORK/AppDir"
install -Dm755 "$BIN" "$APPDIR/usr/bin/herald-desktop"
install -Dm644 "$DESKTOP" "$APPDIR/async-herald.desktop"
install -Dm644 "$LOGO" "$APPDIR/async-herald.png"
install -Dm644 "$LOGO" "$APPDIR/usr/share/icons/hicolor/256x256/apps/async-herald.png"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/herald-desktop" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

TOOL="$WORK/appimagetool"
curl -fsSL -o "$TOOL" \
  "https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_VERSION}/appimagetool-${ARCH}.AppImage"
chmod +x "$TOOL"
APPIMAGE="dist/Async-Herald-${VERSION}-${ARCH}.AppImage"
# Sans FUSE (CI, conteneur), l'outil s'extrait avant de s'exécuter.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$TOOL" --no-appstream "$APPDIR" "$APPIMAGE"
chmod +x "$APPIMAGE"

echo "Version $VERSION"
sha256sum "$DEB" "$APPIMAGE"
