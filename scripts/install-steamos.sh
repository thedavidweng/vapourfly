#!/usr/bin/env bash
# Install Vapourfly for the current user on SteamOS (Steam Deck) or any
# Linux desktop. No root needed: SteamOS keeps / read-only.
#
# Usage, from the extracted Linux release archive in Desktop Mode:
#   ./install-steamos.sh            install and add to Steam (when possible)
#   ./install-steamos.sh --no-steam install only
#
# Installs:
#   ~/.local/bin/vapourfly, ~/.local/bin/vapourfly-gui
#   ~/.local/share/applications/vapourfly.desktop
# and, on SteamOS, adds the GUI to Steam as a non-Steam game with
# steamos-add-to-steam so it shows up in Game Mode.

set -euo pipefail

add_to_steam=1
for arg in "$@"; do
    case "$arg" in
        --no-steam) add_to_steam=0 ;;
        -h|--help)
            sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown option: $arg" >&2
            exit 2
            ;;
    esac
done

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin_dir="${HOME}/.local/bin"
app_dir="${XDG_DATA_HOME:-${HOME}/.local/share}/applications"

find_file() {
    for candidate in "$here/$1" "$here/../$1" "$here/../target/release/$(basename "$1")"; do
        if [ -f "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

gui="$(find_file vapourfly-gui)" || { echo "vapourfly-gui not found next to this script." >&2; exit 1; }
cli="$(find_file vapourfly)" || cli=""
desktop_template="$(find_file packaging/linux/vapourfly.desktop || find_file vapourfly.desktop)" || {
    echo "vapourfly.desktop not found next to this script." >&2
    exit 1
}

mkdir -p "$bin_dir" "$app_dir"
install -m 755 "$gui" "$bin_dir/vapourfly-gui"
if [ -n "$cli" ]; then
    install -m 755 "$cli" "$bin_dir/vapourfly"
fi

desktop="$app_dir/vapourfly.desktop"
sed "s|@BIN@|$bin_dir/vapourfly-gui|" "$desktop_template" > "$desktop"
chmod 644 "$desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$app_dir" >/dev/null 2>&1 || true

echo "Installed Vapourfly to $bin_dir"
echo "Desktop entry: $desktop"

if [ "$add_to_steam" -eq 1 ]; then
    if command -v steamos-add-to-steam >/dev/null 2>&1; then
        steamos-add-to-steam "$desktop"
        echo "Added Vapourfly to Steam. Find it under Non-Steam in Game Mode."
    else
        echo "steamos-add-to-steam is not available. In Steam, use"
        echo "Games > Add a Non-Steam Game to My Library and pick Vapourfly."
    fi
fi

case ":${PATH}:" in
    *":$bin_dir:"*) ;;
    *) echo "Note: $bin_dir is not on PATH. Add it to use the vapourfly CLI from a terminal." ;;
esac
