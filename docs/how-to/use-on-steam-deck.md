# Use Vapourfly on a Steam Deck

Vapourfly runs on SteamOS in both Desktop Mode and Game Mode, and the whole
GUI can be driven with the Deck's controls. This guide installs it, adds it
to Steam, and explains the one thing that works differently on a Deck:
writing to Steam.

## 1. Install in Desktop Mode

1. Hold the power button and choose **Switch to Desktop**.
2. Download the Linux archive (`vapourfly-linux-x86_64.tar.gz`) from
   [GitHub Releases](https://github.com/thedavidweng/vapourfly/releases) and
   extract it, for example into `~/Downloads`.
3. Open **Konsole** in the extracted folder and run:

   ```bash
   ./install-steamos.sh
   ```

The script needs no `sudo` (the SteamOS system partition stays read-only).
It copies `vapourfly` and `vapourfly-gui` into `~/.local/bin`, writes a
desktop entry to `~/.local/share/applications/`, and calls
`steamos-add-to-steam` so Vapourfly appears in your library under
**Non-Steam**. Pass `--no-steam` to skip that last step.

If you prefer to add it by hand: in Steam choose **Games → Add a Non-Steam
Game to My Library**, then pick Vapourfly (or browse to
`~/.local/bin/vapourfly-gui`).

## 2. Controller setup

Leave Steam Input on for Vapourfly (the default for non-Steam games) with
the **Gamepad** template. Vapourfly then reads Steam's virtual gamepad and
ignores the physical devices Steam lists in
`SDL_GAMECONTROLLER_IGNORE_DEVICES`, so you never get doubled input.

| Control | Action |
|---|---|
| D-pad / left stick | Move focus (hold to repeat) |
| A | Select the focused item |
| B | Close the open sheet, dialog or menu; otherwise go back to the sidebar |
| LB / RB | Previous / next page in the sidebar |
| LT / RT | Scroll a screen up / down |
| Right stick | Smooth scroll |
| Y | Jump to Library search |
| X | Show Steam's on-screen keyboard |
| Start (≡) | Open Settings |
| Select (⧉) | Show or hide the sidebar |

Navigation follows the Deck UI. In the sidebar, Up and Down move between
pages and A or Right opens one. In the Library grid the D-pad moves card by
card and row by row, and the list scrolls to keep the focused card in view.
While you use the controller, a footer shows the button hints; moving the
mouse or touching the trackpad hides it.

Text fields: press A on a field (or X anywhere) to bring up the on-screen
keyboard. Steam + X also works.

**Settings → Controller** shows the controller Vapourfly is reading and the
session it detected (for example *Steam Deck · Game Mode*).

To turn controller input off, launch with `VAPOURFLY_NO_GAMEPAD=1`.

## 3. Game Mode

Launched from Game Mode, Vapourfly opens fullscreen at the Deck's
1280×800 and uses a shorter Library hero, so the Recently played shelf starts
on the first screen. Browsing, filtering, Discover, Recommendations and
playlists all work here.

## 4. Writing to Steam

Vapourfly refuses to write while Steam is running, because Steam can
overwrite the collections file and undo the change (see
[Steam file safety](../reference/STEAM_FILE_SAFETY.md)). In Game Mode Steam
is always running, so the confirmation dialog warns you before a write would
be refused.

To write safely:

1. Switch to Desktop Mode.
2. Exit Steam: right-click the Steam icon in the system tray and choose
   **Exit Steam**.
3. Run Vapourfly from the application menu (**Games → Vapourfly**) and
   confirm the write. A backup is made first, as always.
4. Return to Game Mode. Steam picks up the new collections when it starts.

Settings → **Write while Steam is running** removes the guard. Leave it off
unless you know Steam will not overwrite the file.

## Preview the Deck layout on another computer

```bash
cargo run -p vapourfly-gui -- --ui-demo --deck
```

`--deck` opens a 1280×800 window with the Steam Deck session profile, so you
can check layout and controller navigation with any Xbox-style gamepad.
