#!/usr/bin/env python3
"""Import only the pinned signed executable; package tagged metadata separately."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--arch", choices=["arm64", "amd64"], required=True)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    target = {"arm64": "aarch64-apple-darwin", "amd64": "x86_64-apple-darwin"}[args.arch]
    if args.target != target or os.environ.get("GITHUB_REF_NAME") != "v1.0.1":
        raise SystemExit("input is authorized only for the matching v1.0.1 macOS target")
    record = json.loads(Path("release/macos-1.0.1.json").read_text())
    item = record["architectures"][args.arch]
    with tempfile.TemporaryDirectory() as temporary:
        subprocess.run([
            "gh", "release", "download", record["input_tag"],
            "--repo", "rioriost/postgresem", "--pattern", item["archive"],
            "--dir", temporary,
        ], check=True)
        archive = Path(temporary) / item["archive"]
        if hashlib.sha256(archive.read_bytes()).hexdigest() != item["sha256"]:
            raise SystemExit("notarized input archive digest mismatch")
        with tarfile.open(archive) as bundle:
            member = bundle.getmember(item["archive"].removesuffix(".tar.gz") + "/postgresem")
            if not member.isfile():
                raise SystemExit("executable must be a regular file")
            binary = bundle.extractfile(member).read()
        if hashlib.sha256(binary).hexdigest() != item["binary_sha256"]:
            raise SystemExit("notarized executable digest mismatch")
        output = Path("target") / target / "release" / "postgresem"
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(binary)
        output.chmod(0o755)
        subprocess.run(["codesign", "--verify", "--strict", str(output)], check=True)
        signature = subprocess.run(
            ["codesign", "-dv", "--verbose=4", str(output)],
            check=True, capture_output=True, text=True,
        ).stderr
        if "TeamIdentifier=" + record["team_id"] not in signature:
            raise SystemExit("unexpected signing team")
        build = subprocess.check_output(["xcrun", "vtool", "-show-build", str(output)], text=True)
        if not any(line.split() == ["sdk", record["sdk"]] for line in build.splitlines()):
            raise SystemExit("expected macOS 27 SDK")
        print("Verified pinned notarized macOS executable:", args.arch)


if __name__ == "__main__":
    main()
