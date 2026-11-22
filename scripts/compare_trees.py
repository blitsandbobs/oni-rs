"""Compare two converted trees as data.

JSON files are compared by value: numbers at f32 precision (so `0.1` and
`0.10000000149011612` match), object keys in any order. glTF files are
compared by their JSON chunk (the same way) and their binary chunks byte for
byte. Use it to check that a converter change leaves its output unchanged:

    uv run python scripts/compare_trees.py BASE NEW ["**/*.json" "**/*.glb" ...]
"""

import glob
import json
import os
import struct
import sys
from typing import Any


def _f32(x: float) -> float:
    return struct.unpack("f", struct.pack("f", x))[0]


def same(a: Any, b: Any, path: str) -> str | None:
    """None when `a` and `b` are the same data, else where they differ."""
    if isinstance(a, bool) or isinstance(b, bool):
        return None if a == b else f"{path}: {a!r} != {b!r}"
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        try:
            equal = _f32(a) == _f32(b)
        except OverflowError:
            equal = a == b
        return None if equal else f"{path}: {a!r} != {b!r}"
    if type(a) is not type(b):
        return f"{path}: {type(a).__name__} != {type(b).__name__}"
    if isinstance(a, dict):
        if set(a) != set(b):
            return f"{path}: keys {sorted(set(a) ^ set(b))}"
        for k in a:
            if (r := same(a[k], b[k], f"{path}.{k}")) is not None:
                return r
        return None
    if isinstance(a, list):
        if len(a) != len(b):
            return f"{path}: length {len(a)} != {len(b)}"
        for i, (x, y) in enumerate(zip(a, b)):
            if (r := same(x, y, f"{path}[{i}]")) is not None:
                return r
        return None
    return None if a == b else f"{path}: {a!r} != {b!r}"


def _glb_chunks(path: str) -> list[tuple[int, bytes]]:
    with open(path, "rb") as f:
        data = f.read()
    chunks, at = [], 12
    while at + 8 <= len(data):
        length, kind = struct.unpack_from("<II", data, at)
        chunks.append((kind, data[at + 8 : at + 8 + length]))
        at += 8 + length
    return chunks


def compare(base_file: str, new_file: str, rel: str) -> str | None:
    """None when the two files hold the same data, else the difference."""
    if rel.endswith(".glb"):
        a, b = _glb_chunks(base_file), _glb_chunks(new_file)
        ja = json.loads(a[0][1].rstrip(b" \0"))
        jb = json.loads(b[0][1].rstrip(b" \0"))
        if (r := same(ja, jb, rel)) is not None:
            return r
        return None if a[1:] == b[1:] else f"{rel}: binary chunks differ"
    with open(base_file) as fa, open(new_file) as fb:
        return same(json.load(fa), json.load(fb), rel)


def main() -> int:
    base, new = sys.argv[1], sys.argv[2]
    patterns = sys.argv[3:] or ["**/*.json"]
    files = bad = 0
    for pattern in patterns:
        for f in sorted(glob.glob(os.path.join(base, pattern), recursive=True)):
            rel = os.path.relpath(f, base)
            files += 1
            g = os.path.join(new, rel)
            result = "missing" if not os.path.exists(g) else compare(f, g, rel)
            if result is not None:
                print(f"DIFF {rel}: {result}")
                bad += 1
    print(f"{files} files, {bad} differ")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
