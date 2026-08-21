# Getting Started

A first walk through Vapourfly: verify your setup, read your library, and
look at your Steam collections. No files are written in this tutorial.

Takes about 5 minutes. You need the `vapourfly` CLI installed and Steam
installed on this machine. If you have not installed it yet, see the
[README](../../README.md#installation) first.

> Output blocks in the guides were captured from real runs of vapourfly
> 0.2.0 on macOS against an 865-game library. Home paths, account names,
> and SteamIDs are lightly redacted; everything else is verbatim.

## Step 1: Check your setup

Run the doctor:

```bash
vapourfly doctor
```

```text
Vapourfly Doctor
================
Steam dir:     [REDACTED]/Steam
Accounts:      1 detected
Selected:      <account-name> (***) [***8212]
Libraries:     1
Cloud storage: available
Cache root:    [REDACTED]/cache

Credentials
-----------
IGDB:          not configured
RAWG:          not configured
Steam Web API: configured (instant name resolution)
```

Reading the report:

- **Steam dir** -- where Vapourfly found your Steam installation. Override
  with `--steam-dir` or `VAPOURFLY_STEAM_DIR` if this is wrong.
- **Selected** -- which Steam account commands will act on. With multiple
  accounts, pick one with `--account <id>` or `vapourfly settings set account`.
- **Cloud storage** -- must say `available` before any write command can run.
  This is the file that holds your Steam collections.
- **Cache root** -- where cached external metadata (ProtonDB ratings, HLTB
  times, store prices) lives.
- **Credentials** -- optional API keys. Everything works without them;
  some enriched metadata needs them (see
  [Configure API credentials](../how-to/configure-api-credentials.md)).

If anything looks wrong here, fix it before continuing -- every other
command depends on these paths.

## Step 2: Take your first scan

Scan reads your local Steam files and prints what it found:

```bash
vapourfly scan --format table
```

```text
AppID      Name                                     Installed  Playtime     Collections 
--------------------------------------------------------------------------------------
10         App 10                                   no         1038         0           
10150      App 10150                                no         4712         3           
10180      App 10180                                no         1035         0           
10190      App 10190                                no         1064         1           
1035510    App 1035510                              no         47           0           
1038250    App 1038250                              no         0            1           
105450     App 105450                               no         1            0           
1067540    App 1067540                              no         364          0           

[...]

865 games found
Warnings:
  [unresolved_names] 865 games have placeholder names (no local name source); names backfill from Steam Store hydration when online
```

Columns are AppID, Name, Installed, Playtime (minutes), and how many
collections contain the game.

The `unresolved_names` warning in this capture means the scan ran without a
usable game-name source, so names show as `App <id>` placeholders. Names
come back from cache hydration or Steam Store lookups -- run a workflow
command like `vapourfly junk preview`, populate the cache with
`vapourfly cache refresh --source steam-store`, and they resolve. This is
the degradation-first design described in
[How library hydration works](../explanation/hydration-model.md): the scan
itself is instant and local; richer data fills in afterwards.

For scripting, use JSON instead:

```bash
vapourfly scan --format json
```

```json
{
  "account": "<account-name>",
  "games": [
    {
      "app_id": 10,
      "collections": [],
      "installed": false,
      "is_hidden": false,
      "name": "App 10",
      "playtime_2wks_minutes": null,
      "playtime_minutes": 1038
    }
  ]
}
```

(The real output lists all games under `"games"`; one entry is shown.)

## Step 3: Look at your collections

Collections are the Steam-native groups you already have:

```bash
vapourfly collections list
```

```text
Name                           ID         Apps      
-------------------------------------------------------
ACT                            from-tag-ACT 17        
FPS                            uc-sNiD6UeYENFN 8         
GAL                            from-tag-GAL 5         
Independent                    from-tag-Independent 24        
Open World                     from-tag-Open World 33        
Pixel                          from-tag-Pixel 14        
RAC                            from-tag-RAC 6         
RTS                            from-tag-RTS 5         
Roguelike                      from-tag-Roguelike 8         
TPS                            from-tag-TPS 22        
喜加一                            from-tag-喜加一 394       
收藏夹                            favorite   2         

Hidden: 0 apps
```

Each row shows the display name, the internal collection ID used when
syncing, and the member count. Hidden games are reported separately at the
bottom.

## Checkpoint

You can now read your library from the command line. From here:

- Flag the games you will never play:
  [Purge junk from your library](../how-to/purge-junk.md)
- Pick something for tonight's session:
  [Plan a Deck session](../how-to/plan-deck-session.md)
- Build and share curated lists:
  [Share and sync playlists](../how-to/share-and-sync-playlists.md)
- Full command surface: [Command reference](../reference/COMMANDS.md)
