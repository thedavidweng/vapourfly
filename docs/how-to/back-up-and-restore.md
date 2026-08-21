# Back Up and Restore Steam Files

Every confirmed Vapourfly write creates a timestamped backup of the Steam
file it is about to modify, stored next to the original. This guide covers
listing those backups and restoring one when a write turns out to be
unwanted.

The full write contract -- what gets written, atomicity, the
Steam-running guard -- is specified in
[Steam file safety](../reference/STEAM_FILE_SAFETY.md).

## 1. List available backups

```bash
vapourfly backup list
```

```text
No backups found.
```

This machine had not made any confirmed writes yet. After your first
`junk apply --confirm`, `sync collection --confirm`, or similar, the same
command lists one row per backup with its path, creation time, and the
first 8 characters of its SHA-256 hash. For scripting, use
`vapourfly backup list --format json`.

## 2. Restore a backup

Restoring is itself a write, so it follows the same two-step pattern.
Preview first:

```bash
vapourfly backup restore /path/to/cloud-storage-namespace-1.json.vapourfly-backup-20260624T120000Z-a1b2c3d4.json --dry-run
```

Then confirm:

```bash
vapourfly backup restore /path/to/cloud-storage-namespace-1.json.vapourfly-backup-20260624T120000Z-a1b2c3d4.json --confirm
```

The restore verifies the backup's SHA-256 hash before writing. Close Steam
first, as with every write.

## 3. Tune how many backups are kept

The most recent 5 backups are retained per file by default:

```bash
vapourfly settings set backup_retention_count 10
```

## If a write fails midway

Writes are atomic (temporary file, fsync, rename). If anything fails after
the backup is created, Vapourfly attempts an automatic restore from that
backup before reporting the error. If you ever end up with a corrupted
cloud storage file despite this, restore the newest backup as shown above.

## Next steps

- What exactly gets backed up, and the backup naming scheme:
  [Steam file safety](../reference/STEAM_FILE_SAFETY.md)
- Preview before you commit: [Purge junk from your library](purge-junk.md)
  and [Share and sync playlists](share-and-sync-playlists.md) both follow
  the dry-run/confirm pattern.
