"""Stage CLI binaries from successful same-repository release-branch push CI."""
import hashlib
import io
import json
import os
import re
import subprocess
import tempfile
import zipfile
from pathlib import Path

TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
)
MAX_BINARY = 64 * 1024 * 1024


def api(path):
    return subprocess.check_output(["gh", "api", path], timeout=90)


def select_run(runs, repo, sha, branch):
    return next((run for run in runs if
                 run["event"] == "push" and run["conclusion"] == "success"
                 and run["status"] == "completed" and run["head_sha"] == sha
                 and run["head_branch"] == branch
                 and run["path"] == ".github/workflows/ci.yml"
                 and run["head_repository"]["full_name"] == repo
                 and run["repository"]["full_name"] == repo), None)


def verify_archive(archive, artifact, target, sha, tag, output):
    if "sha256:" + hashlib.sha256(archive).hexdigest() != artifact["digest"]:
        raise ValueError("archive digest mismatch")
    name = "everycli-" + target
    checksum = name + ".sha256"
    with zipfile.ZipFile(io.BytesIO(archive)) as z:
        if sorted(z.namelist()) != sorted([name, checksum, "candidate.json"]):
            raise ValueError("unexpected artifact contents")
        for member, limit in [(name, MAX_BINARY), (checksum, 256), ("candidate.json", 1024)]:
            if not 0 < z.getinfo(member).file_size <= limit:
                raise ValueError("invalid artifact size")
        metadata = json.loads(z.read("candidate.json"))
        if metadata != {"sha": sha, "target": target, "version": tag[1:], "implementation": "rust"}:
            raise ValueError("candidate provenance or native version mismatch")
        binary = z.read(name)
        digest = hashlib.sha256(binary).hexdigest()
        if z.read(checksum) != f"{digest}  {name}\n".encode():
            raise ValueError("binary checksum mismatch")
    (output / name).write_bytes(binary)
    (output / checksum).write_text(f"{digest}  {name}\n")


def stage(repo, tag, sha, output):
    if not re.fullmatch(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", tag):
        raise ValueError("invalid version tag")
    branch = "release/" + tag
    base = "repos/" + repo + "/actions"
    runs = json.loads(api(f"{base}/workflows/ci.yml/runs?event=push&status=success&branch=release%2F{tag}&head_sha={sha}"))
    run = select_run(runs["workflow_runs"], repo, sha, branch)
    if run is None:
        raise ValueError("no completed candidate run for this commit")
    artifacts = json.loads(api(f"{base}/runs/{run['id']}/artifacts?per_page=100"))["artifacts"]
    with tempfile.TemporaryDirectory(dir=output.parent) as temp:
        staging = Path(temp) / "dist"
        staging.mkdir()
        for target in TARGETS:
            matches = [a for a in artifacts if a["name"] == "everycli-" + target]
            if len(matches) != 1 or matches[0]["expired"]:
                raise ValueError("missing, ambiguous or expired artifact: " + target)
            artifact = matches[0]
            if not 0 < artifact["size_in_bytes"] <= MAX_BINARY:
                raise ValueError("invalid download size")
            archive = api(f"{base}/artifacts/{artifact['id']}/zip")
            verify_archive(archive, artifact, target, sha, tag, staging)
        staging.rename(output)
    return run["id"]


if __name__ == "__main__":
    try:
        run_id = stage(os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_REF_NAME"],
                       os.environ["GITHUB_SHA"], Path("reviewed"))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, zipfile.BadZipFile) as exc:
        print(f"Using native release builds: {exc}")
    else:
        print(f"Verified all four CLI binaries from CI run {run_id}")
        with open(os.environ["GITHUB_OUTPUT"], "a") as out:
            print("found=true", file=out)
