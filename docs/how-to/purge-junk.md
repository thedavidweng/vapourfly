# Purge Junk from Your Library

You own hundreds of games and will never play many of them -- demos,
abandoned freebies, one-minute curiosities. This guide shows how to find
those candidates with an explainable verdict, then either tag them into a
Steam collection or hide them.

Every step except the last is read-only. The final write steps follow the
[safety model](../reference/STEAM_FILE_SAFETY.md): dry-run preview, then a
confirmed write that creates a timestamped backup first.

## 1. Preview junk candidates

```bash
vapourfly junk preview
```

```text
AppID      Name                               Playtime Confidence  Classification
--------------------------------------------------------------------------------------
10         Counter-Strike                     1038 min        33%  ok
10150      Prototype                          4712 min        66%  ok
1035510    Ultimate Zombie Defense              47 min        66%  ok

[...]

1168280    Resident Evil 2 "R.P.D. Demo"         6 min        66%  junk — low playtime (6m), short story (0.5h)
1712830    Baldi's Basics Classic Remast…        6 min        66%  junk — low playtime (6m), short story (1.2h)
203160     Tomb Raider                           1 min        66%  junk — low playtime (1m), short story (1.3h)
209670     Cortex Command                        1 min        66%  junk — low playtime (1m), short story (1.7h)
225600     Blade Symphony                        1 min        66%  junk — low playtime (1m), short story (0.2h)
250600     The Plan                              3 min        66%  junk — low playtime (3m), short story (0.1h)
255520     Viscera Cleanup Detail: Shado…        3 min        66%  junk — low playtime (3m), short story (1.6h)
261820     Estranged: Act I                      2 min        66%  junk — low playtime (2m), short story (1.8h)

[...]

865 games scanned, 19 junk candidates (mode: default)
```

Every row tells you *why*: which signals matched (`low playtime`,
`short story`, `low rating`), and a confidence score reflecting how much
data was available for that game. Games are listed as `ok` when they do not
qualify. What the modes mean and how confidence is computed is explained in
[junk classification](../explanation/junk-classification.md).

## 2. Tune the mode if needed

The default mode balances recall against false positives. If it is too
noisy or too quiet for your taste:

```bash
# Only flag games where every available signal agrees
vapourfly junk preview --strict

# Flag with fewer signals
vapourfly junk preview --aggressive
```

On the library used for these captures, all three modes reported:

```text
865 games scanned, 19 junk candidates (mode: strict)
```

```text
865 games scanned, 19 junk candidates (mode: aggressive)
```

Use `--format json` on any preview for machine-readable output.

## 3. Apply the classification to a Steam collection

Preview exactly what would be written first (use whatever collection name
you prefer; the capture below used `vf-docs-junk` to avoid touching a real
collection):

```bash
vapourfly junk apply --collection vf-docs-junk --dry-run
```

```text
Junk Apply
==========
Collection: vf-docs-junk
Junk games: 19

Diff:
  Collection 'vf-docs-junk': created
  App IDs to add: 19
  Unchanged entries: 13

Dry run complete. No changes made.
```

When the diff looks right, execute it. Close Steam first -- Vapourfly
refuses to write while Steam is running unless you pass
`--allow-steam-running`:

```bash
vapourfly junk apply --collection junk --confirm
```

## 4. Or move them to the hidden collection

Instead of a custom collection, add the candidates to Steam's hidden
group so they disappear from your library view:

```bash
vapourfly junk hide --dry-run
vapourfly junk hide --confirm
```

Omitting both `--dry-run` and `--confirm` is an error, by design:

```text
Error: must specify either --dry-run or --confirm
```

If anything looks wrong after a confirmed write, restore the automatic
backup: [Back up and restore Steam files](back-up-and-restore.md).
