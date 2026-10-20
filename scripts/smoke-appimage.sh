#!/usr/bin/env bash
set -euo pipefail
trap 'echo "AppImage smoke failed at line $LINENO" >&2; if [ -f "${work:-}/startup.log" ]; then cat "$work/startup.log" >&2; fi' ERR
image="$(realpath "${1:?AppImage required}")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
[ "$(id -u)" -ne 0 ]
expected='gh-releases-zsync|thedavidweng|vapourfly|latest|vapourfly-*-x86_64.AppImage.zsync'
[ "$("$image" --appimage-updateinformation 2>&1)" = "$expected" ]
test -s "$image.zsync"
unsquashfs -offset "$("$image" --appimage-offset)" -d "$work/app" "$image" >/dev/null
python3 - "$work/app" <<'PY'
import pathlib, stat, sys
root = pathlib.Path(sys.argv[1])
for name in ('AppRun', 'usr/bin/vapourfly-gui'):
    path = root / name
    assert path.is_file() and stat.S_IMODE(path.stat().st_mode) & 0o555 == 0o555, path
    for parent in path.parents:
        assert stat.S_IMODE(parent.stat().st_mode) & 0o111 == 0o111, parent
        if parent == root:
            break
PY
chmod 755 "$work"
mkdir "$work/home"
export HOME="$work/home" XDG_CONFIG_HOME="$work/home/config" XDG_DATA_HOME="$work/home/data" XDG_CACHE_HOME="$work/home/cache" LIBGL_ALWAYS_SOFTWARE=1
setsid sudo unshare --net -- runuser -u "$(id -un)" -- \
  env HOME="$HOME" XDG_CONFIG_HOME="$XDG_CONFIG_HOME" XDG_DATA_HOME="$XDG_DATA_HOME" XDG_CACHE_HOME="$XDG_CACHE_HOME" DISPLAY="$DISPLAY" XAUTHORITY="${XAUTHORITY:-}" LIBGL_ALWAYS_SOFTWARE=1 \
  "$work/app/AppRun" >"$work/startup.log" 2>&1 &
pid=$!
trap 'sudo kill -- "-$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; rm -rf "$work"' EXIT
for _ in $(seq 1 30); do
  if ! kill -0 "$pid" 2>/dev/null; then cat "$work/startup.log"; exit 1; fi
  if xdotool search --onlyvisible --name '^Vapourfly$' >"$work/windows" 2>/dev/null; then
    sleep 3
    kill -0 "$pid"
    import -window "$(cat "$work/windows")" "${RUNNER_TEMP:-/tmp}/vapourfly-appimage.png"
    cat "$work/startup.log"
    echo 'Vapourfly AppImage displayed its window offline as non-root'
    exit 0
  fi
  sleep 1
done
cat "$work/startup.log"
exit 1
