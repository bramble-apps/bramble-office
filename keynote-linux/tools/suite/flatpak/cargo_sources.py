"""Flatpak sources for a Cargo.lock's crates, so flatpak-builder can build offline (Flathub builds
without network). Every crates.io package becomes a .crate download unpacked into cargo/vendor/,
with the checksum file cargo wants; a config.toml points cargo at that folder. Writes JSON for the
manifest's `sources:` (what flatpak-cargo-generator.py makes, for registry crates only: the suite
has no git dependencies).

  cargo_sources.py <Cargo.lock> <out.json>
"""
import json
import sys
import tomllib

CRATES = "https://static.crates.io/crates"


def main():
    lock_path, out = sys.argv[1], sys.argv[2]
    with open(lock_path, "rb") as f:
        lock = tomllib.load(f)
    sources = []
    for p in lock["package"]:
        src = p.get("source", "")
        if not src:
            continue  # the workspace's own crates and path dependencies
        if not src.startswith("registry+"):
            sys.exit(f"{p['name']} {p['version']}: only crates.io dependencies are handled ({src})")
        name, ver, sha = p["name"], p["version"], p["checksum"]
        dest = f"cargo/vendor/{name}-{ver}"
        sources.append({
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": f"{CRATES}/{name}/{name}-{ver}.crate",
            "sha256": sha,
            "dest": dest,
        })
        sources.append({
            "type": "inline",
            "contents": json.dumps({"package": sha, "files": {}}),
            "dest": dest,
            "dest-filename": ".cargo-checksum.json",
        })
    sources.append({
        "type": "inline",
        "contents": '[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n'
                    '[source.crates-io]\nreplace-with = "vendored-sources"\n',
        "dest": "cargo",
        "dest-filename": "config.toml",
    })
    with open(out, "w") as f:
        json.dump(sources, f, indent=2)
        f.write("\n")
    print(f"{out}: {len(sources) // 2} crates")


if __name__ == "__main__":
    main()
