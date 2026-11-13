#!/usr/bin/env python3
"""Check compiled configuration assets against the explicit container allowlist."""
import re
from pathlib import Path

root = Path(__file__).resolve().parent.parent
allow = set((root / ".dockerignore").read_text().splitlines())
assets = set()
for source in (root / "src").rglob("*.rs"):
    for relative in re.findall(r'include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)', source.read_text()):
        asset = (source.parent / relative).resolve()
        if asset.is_relative_to(root / "config"):
            path = asset.relative_to(root).as_posix()
            if not asset.is_file() or "!" + path not in allow:
                raise SystemExit(f"Compiled config missing from container allowlist: {path}")
            assets.add(path)
if not assets:
    raise SystemExit("No compiled configuration references found")
print(f"Container allowlist includes all {len(assets)} compiled configuration assets")
