# Steam File Safety

Vapourfly modifies Steam configuration files to manage collections, hidden state, and other metadata. This document explains which files are written, how backups work, and the safety checks that prevent data loss.

## Write Targets

Vapourfly writes to a single file per account:

```
<steam_dir>/userdata/<account_id_or_steam_id64>/config/cloudstorage/cloud-storage-namespace-1.json
```

This is Steam's cloud storage file. It contains user-defined collections, hidden app state, and other per-account metadata. Vapourfly reads and writes this file using the same JSON schema that Steam uses internally.

**Files Vapourfly never writes to:**
- `sharedconfig.vdf` -- legacy collection storage, not used by modern Steam
- `localconfig.vdf` -- per-app settings and playtime (read-only)
- `loginusers.vdf` -- account metadata (read-only)
- `libraryfolders.vdf` -- library folder configuration (read-only)
- `appmanifest_*.acf` -- per-game install metadata (read-only)
- `appinfo.vdf` -- app metadata cache (read-only)
- `librarycache` -- app library cache data (read-only)

## Safety Checks

Before any write, Vapourfly performs these checks in order:

1. **Steam process detection.** If the Steam client is running, the write is refused with an error. Close Steam before making changes. Detection uses `pgrep` on macOS/Linux and `tasklist` on Windows.

2. **Target file existence.** The target cloud storage file must exist. If it is missing, Vapourfly cannot create it (Steam must create it first).

3. **Parent directory existence.** The parent directory must exist.

4. **Write permissions (Unix).** The parent directory must have at least one write permission bit set. This catches read-only mounts and `chmod a-w` directories.

If any check fails, the write is aborted before any file is modified.

## Backup Strategy

Every write operation follows a write-ahead pattern:

1. **Confirm** the target file still matches the expected pre-write SHA-256 hash.
2. **Backup** the target before any mutation.
3. **Write** to a temporary file in the same directory.
4. **fsync** the temporary file.
5. **Rename** the temporary file atomically over the target.
6. **fsync** the parent directory (best-effort, platform-dependent).
7. **Verify** the written file by re-reading and checking its SHA-256 hash.
8. **Prune** old backups, keeping only the most recent N (default: 5).

If any step after backup creation fails, an automatic restore is attempted from the backup.

### Backup File Naming

Backups are stored alongside the original file with this naming pattern:

```
cloud-storage-namespace-1.json.vapourfly-backup-YYYYMMDDTHHMMSSZ-SHA256PREFIX.json
```

Example:

```
cloud-storage-namespace-1.json.vapourfly-backup-20260624T120000Z-a1b2c3d4.json
```

The timestamp is UTC. The SHA-256 prefix (first 8 hex characters) allows verifying backup integrity without reading the full file.

### Listing Backups

```bash
vapourfly backup list
vapourfly backup list --format json
```

### Restoring a Backup

```bash
vapourfly backup restore /path/to/cloud-storage-namespace-1.json.vapourfly-backup-20260624T120000Z-a1b2c3d4.json
```

Restoring copies the backup content over the current cloud storage file. The pre-restore state is itself backed up, so you can always undo a restore.

## Atomic Writes

Vapourfly never writes directly to the target file. Instead:

1. A temporary file is created in the same directory (e.g., `.cloud-storage-namespace-1.json.vapourfly.tmp-12345`).
2. The new content is written to the temporary file.
3. The temporary file is fsynced to ensure data is on disk.
4. The temporary file is renamed over the target (atomic on most filesystems).
5. The parent directory is fsynced (best-effort).

This ensures that if Vapourfly crashes or the system loses power mid-write, the target file is never left in a partially-written state.

## Post-Write Verification

After the atomic rename, Vapourfly re-reads the target file and verifies its SHA-256 hash matches the expected value. If verification fails, an error is reported and the backup can be used to restore.

## Retention

By default, the 5 most recent backups are retained for each target file. Older backups are pruned automatically after each write. You can manually restore any backup before it is pruned.

## Steam Running Detection

Vapourfly detects the Steam client process on each platform:

| Platform | Detection Method |
|---|---|
| macOS | `pgrep -xq steam_osx` |
| Linux | `pidof -s steam` (fallback: `pgrep -xq steam`) |
| Windows | `tasklist /FI "IMAGENAME eq Steam.exe"` |

Detection is best-effort and conservative: if detection fails, Steam is assumed to not be running (to avoid blocking writes unnecessarily). You can override this with the `--allow-steam-running` flag on supported commands (not recommended).

## What Vapourfly Does Not Touch

Vapourfly is designed to work exclusively with Steam's cloud storage collection system. It does not:

- Modify game installation files
- Change game settings or launch options
- Alter Steam client settings
- Write to the Steam registry (Windows)
- Interfere with Steam Cloud sync
- Modify Steam Guard or authentication state
