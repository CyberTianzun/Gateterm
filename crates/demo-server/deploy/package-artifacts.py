#!/usr/bin/env python3
"""Package verified static Linux files into minimal Docker build contexts; no registry access."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import tarfile

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("probe", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    args.destination.mkdir(parents=True, exist_ok=True)
    record = {}
    for name, binary, dockerfile in [
        ("runtime", args.binary, (HERE / "Dockerfile.artifact").read_bytes()),
        ("probe", args.probe, b"FROM scratch\nCOPY probe /probe\nUSER 65532:65532\nENTRYPOINT [\"/probe\"]\n"),
    ]:
        data = binary.read_bytes()
        assert data[:4] == b"\x7fELF" and data[4] == 2 and data[18:20] == b"\x3e\x00", "Expected Linux amd64 ELF"
        path = args.destination / f"{name}-context.tar"
        with tarfile.open(path, "w", format=tarfile.USTAR_FORMAT) as archive:
            for filename, content, mode in [("Dockerfile", dockerfile, 0o644), ("gateterm-demo" if name == "runtime" else "probe", data, 0o555)]:
                info = tarfile.TarInfo(filename)
                info.size, info.mode = len(content), mode
                archive.addfile(info, io.BytesIO(content))
        record[name] = {"binary_sha256": hashlib.sha256(data).hexdigest(), "context_sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "context": path.name}
    profile = (HERE / "seccomp.json").read_bytes()
    (args.destination / "seccomp.json").write_bytes(profile)
    record["seccomp_sha256"] = hashlib.sha256(profile).hexdigest()
    (args.destination / "artifacts.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
