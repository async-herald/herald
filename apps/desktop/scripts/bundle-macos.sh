#!/usr/bin/env bash
# Construit « Herald.app » : l'application macOS avec l'icône Herald dans
# le Dock, le Finder et le sélecteur d'applications (⌘⇥). Un binaire lancé par
# `cargo run` n'a pas de bundle, donc pas d'icône : macOS affiche l'icône
# générique des exécutables.
#
#   scripts/bundle-macos.sh          # → ../../target/release/bundle/Herald.app
#                                    #   + ../../target/release/bundle/Async-Herald-<version>.dmg
#   open "../../target/release/bundle/Herald.app"   (depuis apps/desktop)
set -euo pipefail
cd "$(dirname "$0")/.."

# Nom affiché (Dock, barre des menus, volume du DMG).
NAME="Async Herald"
# Nom du fichier : Homebrew en tire le nom du cask (`brew install herald`). Une
# app déjà installée sous « Async Herald.app » garde son nom de dossier : la
# mise à jour intégrée remplace le contenu de l'app en cours, quel que soit le
# nom de l'app dans le DMG.
BUNDLE="Herald"
BIN="herald-desktop"
IDENTIFIER="fr.itsasync.herald.desktop"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
LOGO="assets/brand/angel.png"
# Cible du workspace Cargo : `target/` à la racine du dépôt.
TARGET_DIR="${CARGO_TARGET_DIR:-../../target}"
OUT="$TARGET_DIR/release/bundle"
APP="$OUT/$BUNDLE.app"

# Binaire universel : Apple Silicon (arm64) et Intel (x86_64) dans un seul
# exécutable, assemblé par lipo. Un seul .dmg, une seule entrée « macos » dans
# l'admin et le même cask pour tous les Mac.
# HERALD_MAC_ARCH=native : seulement l'architecture de la machine (plus rapide en local).
ARCHS=(aarch64-apple-darwin x86_64-apple-darwin)
if [ "${HERALD_MAC_ARCH:-universal}" = native ]; then
  cargo build --release --bin "$BIN"
  BINARY="$TARGET_DIR/release/$BIN"
else
  rustup target add "${ARCHS[@]}" >/dev/null
  slices=()
  for arch in "${ARCHS[@]}"; do
    cargo build --release --target "$arch" --bin "$BIN"
    slices+=("$TARGET_DIR/$arch/release/$BIN")
  done
  BINARY="$TARGET_DIR/release/$BIN-universal"
  mkdir -p "$TARGET_DIR/release"
  lipo -create "${slices[@]}" -output "$BINARY"
  lipo "$BINARY" -verify_arch arm64 x86_64
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/$BIN"

# Icône : jeu de tailles généré depuis le logo (256 px), assemblé en .icns.
ICONSET="$OUT/Herald.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256; do
  sips -z "$size" "$size" "$LOGO" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
done
for size in 16 32 128; do
  double=$((size * 2))
  sips -z "$double" "$double" "$LOGO" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Herald.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>$NAME</string>
  <key>CFBundleDisplayName</key>
  <string>$NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$IDENTIFIER</string>
  <key>CFBundleExecutable</key>
  <string>$BIN</string>
  <key>CFBundleIconFile</key>
  <string>Herald</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

# Signature.
# - Avec HERALD_SIGN_ID (certificat « Developer ID Application: … » du compte
#   Apple Developer, dans le Trousseau) : signature avec runtime renforcé et
#   horodatage, condition de la notarisation (plus d'alerte Gatekeeper).
# - Sinon, signature ad hoc avec l'identifiant de l'app : sans elle, macOS ne
#   mémorise pas « Toujours autoriser » pour l'accès au Trousseau ; Gatekeeper
#   bloque l'app téléchargée au premier lancement.
SIGN_ID="${HERALD_SIGN_ID:-}"
if [ -n "$SIGN_ID" ]; then
  codesign --force --options runtime --timestamp --sign "$SIGN_ID" --identifier "$IDENTIFIER" "$APP"
else
  codesign --force --sign - --identifier "$IDENTIFIER" "$APP"
fi

# Image disque à distribuer : l'app et un raccourci vers Applications, à
# glisser l'un sur l'autre. Nom versionné, à héberger puis à publier dans
# l'admin (onglet « Mises à jour »).
DMG="$OUT/Async-Herald-$VERSION.dmg"
STAGE="$OUT/dmg"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "$NAME $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

# Notarisation (avec HERALD_SIGN_ID et HERALD_NOTARY_PROFILE, profil créé une
# fois par `xcrun notarytool store-credentials`) : Apple vérifie le .dmg, puis
# le ticket est agrafé pour que la vérification marche aussi hors ligne.
if [ -n "$SIGN_ID" ]; then
  codesign --force --timestamp --sign "$SIGN_ID" "$DMG"
  if [ -n "${HERALD_NOTARY_PROFILE:-}" ]; then
    # HERALD_NOTARY_KEYCHAIN : trousseau du profil quand ce n'est pas celui de session (CI).
    xcrun notarytool submit "$DMG" --keychain-profile "$HERALD_NOTARY_PROFILE" \
      ${HERALD_NOTARY_KEYCHAIN:+--keychain "$HERALD_NOTARY_KEYCHAIN"} --wait
    xcrun stapler staple "$DMG"
    spctl --assess --type open --context context:primary-signature --verbose "$DMG"
  else
    echo "HERALD_NOTARY_PROFILE absent : .dmg signé mais non notarisé (alerte Gatekeeper)." >&2
  fi
else
  echo "HERALD_SIGN_ID absent : app signée ad hoc, Gatekeeper bloquera le premier lancement." >&2
fi

echo "$APP"
echo "$DMG"
# Empreinte à saisir dans l'admin (onglet « Mises à jour ») : l'app la vérifie
# avant de s'installer seule.
echo "SHA-256 : $(shasum -a 256 "$DMG" | cut -d' ' -f1)"
