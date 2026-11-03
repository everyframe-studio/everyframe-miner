#!/usr/bin/env python3
"""Release gate: every published CLI version must document its upgrade steps."""
import json
import re
import sys
from pathlib import Path

tag = sys.argv[1]
if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
    raise SystemExit("Invalid release tag")
catalog = json.loads((Path(__file__).resolve().parents[1] / "config/upgrade-notes.json").read_text())
entries = [r for r in catalog["releases"] if r["version"] == tag]
if catalog["schema"] != 1 or len(entries) != 1:
    raise SystemExit("Add this version to config/upgrade-notes.json before publishing")
if not entries[0]["changes"] or entries[0]["workerUpdate"] == "unknown":
    raise SystemExit("Document this release and its worker-update requirement")
print(f"Upgrade notes documented for {tag}")
