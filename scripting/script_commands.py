"""Lists the engine commands the levels' translated scripts use that the port
doesn't implement (no match arm in `crates/oni/src/mission/commands.rs`),
with how often and on which levels. Run from oni-rs:

    python3 scripting/script_commands.py

(Some names are the original scripts' own mistakes — a typo, a call to a
function another level defines; the original skips those as well.)
"""

import collections
import glob
import re

src = open("crates/oni/src/mission/commands.rs").read()
arms = re.finditer(r'^\s*((?:"[a-z0-9_]+"\s*\|\s*)*"[a-z0-9_]+")\s*(?:if [^=]*)?=>', src, re.M)
implemented = {n for m in arms for n in re.findall(r'"([a-z0-9_]+)"', m.group(1))}

levels: dict[str, set[str]] = collections.defaultdict(set)
calls: collections.Counter[str] = collections.Counter()
for path in sorted(glob.glob("../generated/oni/levels/*/scripts.lua")):
    level = path.split("/")[-2]
    for name in re.findall(r'C\["([A-Za-z0-9_]+)"\]', open(path).read()):
        levels[name].add(level)
        calls[name] += 1

missing = sorted((n for n in levels if n not in implemented), key=lambda n: (-len(levels[n]), -calls[n], n))
print(f"{len(levels)} commands used, {len(missing)} not implemented")
for n in missing:
    print(f"{n:34s} {calls[n]:4d} calls   levels {' '.join(sorted(levels[n]))}")
