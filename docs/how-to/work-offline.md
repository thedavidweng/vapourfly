# Work Offline

You are on a plane, a train, or a flaky connection, and you want Vapourfly
to touch zero network endpoints. `--offline` prohibits every network call:
cache refresh, Steam Store price lookups, even the bounded name-resolution
request.

## 1. Check what your cache can cover first

```bash
vapourfly sources status
```

```text
Source          Credentials     Last Success    Entries  Stale    Cached    
---------------------------------------------------------------------------
steam-store     not required    2026-08-13 10:52 720      720      yes       
igdb            missing         n/a             0        0        no        
protondb        not required    2026-08-13 10:30 865      865      yes       
pcgw            not required    2026-08-13 11:26 705      0        yes       
hltb            not required    2026-08-13 11:50 612      0        yes       
rawg            missing         n/a             0        0        no        
```

**Entries** is what the cache holds; **Stale** is how much of it is past
its freshness window (stale entries are still used -- see
[How library hydration works](../explanation/hydration-model.md)). Sources
with `0` entries will simply contribute nothing offline.

## 2. Run your commands with `--offline`

The flag is global; add it to any command:

```bash
vapourfly scan --offline --format table
```

```text
AppID      Name                                     Installed  Playtime     Collections 
--------------------------------------------------------------------------------------
10         App 10                                   no         1038         0           
10150      App 10150                                no         4712         3           
10180      App 10180                                no         1035         0           
10190      App 10190                                no         1064         1           

[...]

  [unresolved_names] 865 games have placeholder names (no local name source); names backfill from Steam Store hydration when online
```

Scanning only reads local Steam files, so its output is identical online or
off. The difference shows in commands that consume external metadata:

```bash
vapourfly junk preview --offline
```

```text
865 games scanned, 19 junk candidates (mode: default)
```

Junk classification ran on cached signals alone. Missing fields are omitted
rather than guessed -- a game with no cached rating is never flagged for
`low rating`, and its confidence score says so.

```bash
vapourfly recommend --offline --minutes 120
```

```text
AppID      Name                                        Score  Reasons
--------------------------------------------------------------------------------------
8650       RACE 07: Andy Priaulx Crowne Plaza Ra…       3.50  low_playtime, time_match
234140     Mad Max                                      3.50  low_playtime, time_match
267940     Glacier 3: The Meltdown                      3.50  low_playtime, time_match
283330     Desert Thunder                               3.50  low_playtime, time_match
301690     Cobi Treasure Deluxe                         3.50  low_playtime, time_match

5 recommendations (865 games scanned)
```

On this machine the offline recommendations match the online run because
everything they used was already cached. With a colder cache you would see
fewer picks and thinner reason lists -- degradation, not failure.

## 3. Know what is blocked

Cache refresh is refused outright in offline mode:

```bash
vapourfly cache refresh --source igdb --offline
```

```text
Error: Cannot refresh cache in offline mode.
```

Writes are unaffected: `--dry-run` and `--confirm` behave exactly as
online, because Steam cloud storage is a local file.

## Next steps

- Fill the cache while you still have connectivity:
  [Configure API credentials](configure-api-credentials.md)
- Where cached data comes from: [API sources](../reference/API_SOURCES.md)
