# How Junk Classification Works

Junk detection answers one question -- *will I ever play this?* -- with a
verdict you can audit. This page explains the signals, the three modes,
and why every decision carries its own evidence. For the commands, see
[Purge junk from your library](../how-to/purge-junk.md).

## The signals

Each game is evaluated against three negative signals:

| Signal | Meaning | Default threshold |
|---|---|---|
| **Low playtime** | You have barely played it | Under 30 minutes |
| **Short completion** | Even if you did play it, it is nearly over already | HLTB main story under 2 hours |
| **Low rating** | External ratings rate it poorly | Below 2.5 out of 5 |

The second signal is the interesting one: a game you have played for 3
minutes is not obviously junk if it is a 40-hour RPG you bounced off; it
*is* obviously junk if it is a 20-minute walking demo you already finished
by accident.

## Three modes, one philosophy

| Mode | Logic | Minimum data |
|---|---|---|
| Default | Low playtime AND at least one other negative signal | 2 data points |
| Strict (`--strict`) | Every available signal must indicate junk | 2 data points |
| Aggressive (`--aggressive`) | Low playtime AND at least one other negative signal | none |

The modes trade recall against false positives:

- **Default** needs corroboration. Low playtime alone never flags a game,
  because an unplayed gem looks identical to an unplayed dud until another
  signal agrees.
- **Strict** exists for trust-but-verify workflows: flag only where the
  evidence is unanimous. It produces fewer candidates and misses games
  whose rating is unknown.
- **Aggressive** drops both corroboration and the data minimum. Use it
  when you want a superset to review by hand, not to act on blindly.

The two-data-point minimum in Default and Strict is deliberate: with a
single signal, classification degenerates to that signal's threshold, and
thresholds are heuristics.

## Confidence is measured, not vibes

Every decision includes a confidence score: the fraction of *possible*
signals for which data was actually available. A verdict backed by
playtime + completion time + rating reports higher confidence than one
resting on playtime alone because the other sources had no entry.

The preview output makes this auditable rather than decorative: each row
shows which signals matched, which were missing, and the score. A 33%
confidence junk flag means "one signal agreed, we could only check three"
-- exactly the kind of row you want to eyeball before running
`junk apply --confirm`.

## Degradation instead of failure

Signals come from external sources (HLTB, IGDB/RAWG ratings) via the local
cache. When a source has no entry for a game, that signal is reported as
missing -- never guessed, never silently treated as passing or failing.
With `--offline` this applies to everything uncached. The consequence:
junk results improve as your cache fills (see
[How library hydration works](hydration-model.md)), and rules that depend
on missing data simply do not match.

## Why explainable output

`junk apply` and `junk hide` modify real Steam files. A black-box "trust
me" classifier writing to your library would be unacceptable; the
per-row evidence exists so the dry-run diff can be judged by a human
before anything is written.
