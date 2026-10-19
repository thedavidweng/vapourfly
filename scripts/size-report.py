#!/usr/bin/env python3
"""Read-only logical-byte inventory of the actual packaged distribution."""
import argparse
import hashlib
import json
from pathlib import Path
import stat
import subprocess
import tomllib


def file_record(path, name):
    mode = path.lstat().st_mode
    if stat.S_ISLNK(mode):
        return {"path": name, "kind": "symlink", "bytes": path.lstat().st_size}
    if not stat.S_ISREG(mode):
        raise ValueError(f"Not a regular file or symlink: {name}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return {"path": name, "kind": "file", "bytes": path.stat().st_size,
            "sha256": digest.hexdigest()}


def inventory(stage):
    if not stage.is_dir():
        raise ValueError("Package stage must be an existing directory")
    records = []
    for path in sorted(stage.rglob("*")):
        if path.is_symlink() or not path.is_dir():
            records.append(file_record(path, path.relative_to(stage).as_posix()))
    for record in records:
        parts = Path(record["path"]).parts
        name = parts[-1]
        if name in ("vapourfly", "vapourfly.exe"):
            category = "cli"
        elif name in ("vapourfly-gui", "vapourfly-gui.exe"):
            category = "gui"
        elif "Frameworks" in parts or name.endswith((".so", ".dylib", ".dll")) or ".so." in name:
            category = "libraries"
        elif "Resources" in parts or "share" in parts:
            category = "resources"
        else:
            category = "other"
        record["category"] = category
    return records


def report(stage, assets, metadata):
    files = inventory(stage)
    categories = {name: sum(r["bytes"] for r in files if r["category"] == name)
                  for name in ("cli", "gui", "libraries", "resources", "other")}
    bundles = {}
    for record in files:
        parts = Path(record["path"]).parts
        for index, part in enumerate(parts):
            if part.endswith(".app"):
                name = "/".join(parts[:index + 1])
                bundles[name] = bundles.get(name, 0) + record["bytes"]
    return {"schema": 1, **metadata, "categories": categories,
            "stage_bytes": sum(r["bytes"] for r in files), "files": files,
            "bundles": bundles,
            "assets": [file_record(p, p.name) for p in assets]}


def comparison(current, baseline):
    keys = ("schema", "target", "rustc", "lock_sha256")
    comparable = baseline is not None and all(current[k] == baseline[k] for k in keys)
    rows = []
    for name, size in {**current["categories"], "stage total": current["stage_bytes"]}.items():
        old = None if baseline is None else (
            baseline["stage_bytes"] if name == "stage total" else baseline["categories"][name])
        rows.append((name, old, size, size - old if comparable else None))
    return comparable, rows


def markdown(current, baseline):
    comparable, rows = comparison(current, baseline)
    lines = [f"## Distribution Size: {current['target']}", "",
             "Logical file bytes, not disk allocation or compressed-size equivalents.", "",
             "| Component | Baseline bytes | Candidate bytes | Delta bytes |",
             "| --- | ---: | ---: | ---: |"]
    for name, old, size, delta in rows:
        lines.append(f"| {name} | {old if old is not None else 'unavailable'} | {size} | "
                     f"{delta if delta is not None else 'not comparable'} |")
    lines += ["", "Comparable toolchain/lock/target: " + ("yes" if comparable else "no"),
              f"Candidate profile: `{current['profile']}` {json.dumps(current['profile_config'], sort_keys=True)}",
              (f"Baseline profile: `{baseline['profile']}` {json.dumps(baseline['profile_config'], sort_keys=True)}"
               if baseline else "Baseline profile: unavailable"),
              "No baseline is manufactured; native startup/scan/controller performance is not measured here.",
              "Byte deltas alone do not establish an optimization win, especially across revisions.",
              "", "### App Bundles", "", "| Bundle | Logical bytes |", "| --- | ---: |"]
    for name, size in current["bundles"].items():
        lines.append(f"| {name} | {size} |")
    lines += [
              "", "### Actual Packaged Assets", "", "| Asset | Bytes |", "| --- | ---: |"]
    for record in current["assets"]:
        lines.append(f"| {record['path']} | {record['bytes']} |")
    lines += ["", "### Largest Staged Files", "", "| File | Bytes |", "| --- | ---: |"]
    for record in sorted(current["files"], key=lambda r: (-r["bytes"], r["path"]))[:20]:
        name = record["path"].replace("|", "&#124;").replace("\n", " ")
        lines.append(f"| {name} | {record['bytes']} |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--asset", type=Path, action="append", default=[])
    parser.add_argument("--target", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--profile", default="release")
    parser.add_argument("--toolchain", required=True)
    parser.add_argument("--lockfile", type=Path, default=Path("Cargo.lock"))
    parser.add_argument("--manifest", type=Path, default=Path("Cargo.toml"))
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    with args.manifest.open("rb") as stream:
        profile = tomllib.load(stream)["profile"][args.profile]
    metadata = {"target": args.target, "revision": args.revision, "profile": args.profile,
                "profile_config": profile,
                "rustc": subprocess.check_output(["rustc", f"+{args.toolchain}", "-Vv"], text=True),
                "lock_sha256": hashlib.sha256(args.lockfile.read_bytes()).hexdigest()}
    current = report(args.stage, args.asset, metadata)
    baseline = json.loads(args.baseline.read_text()) if args.baseline else None
    summary = markdown(current, baseline)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.with_suffix(".json").write_text(json.dumps(current, indent=2) + "\n")
    args.out.with_suffix(".md").write_text(summary)
    print(summary, end="")


if __name__ == "__main__":
    main()
