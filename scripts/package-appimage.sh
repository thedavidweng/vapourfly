#!/usr/bin/env bash
set -euo pipefail

version="${1:?version required}"
binary="${2:-target/release/vapourfly-gui}"
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$root/dist"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
appdir="$work/Vapourfly.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/scalable/apps"
install -m755 "$binary" "$appdir/usr/bin/vapourfly-gui"
sed -e 's|@BIN@|vapourfly-gui|' -e 's|Icon=applications-games|Icon=vapourfly|' \
  "$root/packaging/linux/vapourfly.desktop" > "$appdir/usr/share/applications/vapourfly.desktop"
cp "$root/site/assets/img/mark.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/vapourfly.svg"
curl -fL --retry 3 https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage -o "$work/linuxdeploy"
chmod 755 "$work/linuxdeploy"
export LDAI_UPDATE_INFORMATION='gh-releases-zsync|thedavidweng|vapourfly|latest|vapourfly-*-x86_64.AppImage.zsync'
export LDAI_OUTPUT="$root/dist/vapourfly-${version}-x86_64.AppImage"
cd "$work"
./linuxdeploy --appimage-extract-and-run --appdir "$appdir" \
  --executable "$appdir/usr/bin/vapourfly-gui" \
  --library /usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0 \
  --desktop-file "$appdir/usr/share/applications/vapourfly.desktop" \
  --icon-file "$appdir/usr/share/icons/hicolor/scalable/apps/vapourfly.svg" --output appimage
chmod 755 "$LDAI_OUTPUT"
if [ ! -f "$LDAI_OUTPUT.zsync" ]; then
  mv "$(basename "$LDAI_OUTPUT").zsync" "$LDAI_OUTPUT.zsync"
fi
test -s "$LDAI_OUTPUT.zsync"
test "$("$LDAI_OUTPUT" --appimage-updateinformation 2>&1)" = "$LDAI_UPDATE_INFORMATION"
sha256sum "$LDAI_OUTPUT" "$LDAI_OUTPUT.zsync" > "$LDAI_OUTPUT.sha256"
# Observe the deployed libraries before the temporary AppDir is removed. This
# read-only report never gates packaging and never mutates signed artifacts.
if ! python3 "$root/scripts/size-report.py" --stage "$appdir" --asset "$LDAI_OUTPUT" \
  --target linux-x86_64-appimage --revision "${GITHUB_SHA:-$(git -C "$root" rev-parse HEAD)}" \
  --toolchain "${VAPOURFLY_CI_TOOLCHAIN:-1.99}" --manifest "$root/Cargo.toml" \
  --lockfile "$root/Cargo.lock" --out "$root/dist/size-appimage"; then
  echo "Size inventory failed; no size evidence is claimed." >&2
fi
