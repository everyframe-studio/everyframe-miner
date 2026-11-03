#!/usr/bin/env python3
"""Release gate: every published CLI version must document its upgrade steps."""
import json
import re
import sys
from pathlib import Path

tag = sys.argv[1]
assert re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag), "Invalid release tag"
catalog = json.loads((Path(__file__).resolve().parents[1] / "config/upgrade-notes.json").read_text())
entries = [r for r in catalog["releases"] if r["version"] == tag]
assert catalog["schema"] == 1 and len(entries) == 1, "Add this version to config/upgrade-notes.json before publishing"
assert entries[0]["changes"] and entries[0]["workerUpdate"] != "unknown", "Document this release and its worker-update requirement"
print(f"Upgrade notes documented for {tag}")
