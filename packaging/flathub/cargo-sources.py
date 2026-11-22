#!/usr/bin/env python3
"""Writes cargo-sources.json from Cargo.lock: every crate as a checksummed archive, so the Flathub
build needs no network. Run it from the repository root after changing Cargo.lock."""

import json
import tomllib

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"

with open("Cargo.lock", "rb") as f:
    packages = tomllib.load(f)["package"]

sources = []
for p in packages:
    if "source" not in p:
        continue  # Our own crate.
    if p["source"] != REGISTRY:
        raise SystemExit(f"{p['name']}: only crates.io sources are supported, got {p['source']}")
    name, version, sha = p["name"], p["version"], p["checksum"]
    dest = f"cargo/vendor/{name}-{version}"
    sources.append({"type": "archive", "archive-type": "tar-gzip", "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate", "sha256": sha, "dest": dest})
    sources.append({"type": "inline", "contents": json.dumps({"package": sha, "files": {}}), "dest": dest, "dest-filename": ".cargo-checksum.json"})

config = '[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "cargo/vendor"\n'
sources.append({"type": "inline", "contents": config, "dest": "cargo", "dest-filename": "config.toml"})

with open("packaging/flathub/cargo-sources.json", "w", encoding="utf-8") as f:
    json.dump(sources, f, indent=1)
    f.write("\n")
print(f"{(len(sources) - 1) // 2} crates")
