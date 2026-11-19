import hashlib
import importlib.util
import io
import json
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("candidate", Path(__file__).with_name("release-candidate.py"))
candidate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(candidate)


class Promotion(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.output = Path(self.tmp.name) / "reviewed"
        self.repo = "owner/miner"
        self.tag = "v1.2.3"
        self.sha = "a" * 40
        self.run = dict(id=42, event="push", status="completed", conclusion="success", head_sha=self.sha,
                        path=".github/workflows/ci.yml", head_branch="release/" + self.tag,
                        repository={"full_name": self.repo}, head_repository={"full_name": self.repo})
        self.artifacts = []
        self.files = {}
        for index, target in enumerate(candidate.TARGETS):
            name = "everycli-" + target
            data = name.encode()
            checksum = hashlib.sha256(data).hexdigest()
            self.files[index] = {
                name: data,
                name + ".sha256": f"{checksum}  {name}\n",
                "candidate.json": json.dumps(dict(sha=self.sha, version="1.2.3", implementation="rust", target=target)),
            }
            self.artifacts.append(dict(id=index, name=name, expired=False))

    def stage(self, corrupt_digest=False):
        archives = {}
        for artifact in self.artifacts:
            index = artifact["id"]
            buf = io.BytesIO()
            with zipfile.ZipFile(buf, "w") as z:
                for key, value in self.files[index].items():
                    z.writestr(key, value)
            archives[index] = buf.getvalue()
            artifact.update(size_in_bytes=len(archives[index]),
                            digest="sha256:" + hashlib.sha256(archives[index]).hexdigest())
        if corrupt_digest:
            self.artifacts[-1]["digest"] = "sha256:" + "0" * 64

        def api(path):
            if "/workflows/ci.yml/runs?" in path:
                return json.dumps({"workflow_runs": [self.run]})
            if "/runs/42/artifacts?" in path:
                return json.dumps({"artifacts": self.artifacts})
            return archives[int(path.split("/")[-2])]

        with patch.object(candidate, "api", api):
            return candidate.stage(self.repo, self.tag, self.sha, self.output)

    def test_complete_candidate(self):
        self.assertEqual(self.stage(), 42)
        self.assertEqual(len(list(self.output.iterdir())), 8)
        self.assertFalse((self.output / "candidate.json").exists())

    def test_untrusted_or_other_commit(self):
        for changes in [dict(event="pull_request"), dict(head_branch="main"), dict(head_sha="b"*40),
                        dict(head_repository={"full_name":"fork/miner"}), dict(conclusion="failure"),
                        dict(path=".github/workflows/unrelated.yml")]:
            with self.subTest(changes=changes), patch.dict(self.run, changes), self.assertRaises(ValueError):
                self.stage()
            self.assertFalse(self.output.exists())

    def test_partial_candidate(self):
        self.artifacts.pop()
        with self.assertRaises(ValueError):
            self.stage()
        self.assertFalse(self.output.exists())

    def test_corrupt_expired_or_wrong_target(self):
        with self.assertRaises(ValueError):
            self.stage(corrupt_digest=True)
        with patch.dict(self.artifacts[-1], expired=True), self.assertRaises(ValueError):
            self.stage()
        self.files[3]["candidate.json"] = self.files[0]["candidate.json"]
        with self.assertRaises(ValueError):
            self.stage()
        self.assertFalse(self.output.exists())

    def test_unexpected_member_or_bad_binary(self):
        with patch.dict(self.files[0], {"../escape": "oops"}), self.assertRaises(ValueError):
            self.stage()
        self.files[0][self.artifacts[0]["name"]] = b"bad binary"
        with self.assertRaises(ValueError):
            self.stage()
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
