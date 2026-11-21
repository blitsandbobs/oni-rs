"""Writes a level's shakedown tour (scripting/shakedown.sh --tour): every
trigger volume's centre and every console's use point, as a Lua table."""
import json
import sys

level_dir, out = sys.argv[1], sys.argv[2]


def load(name, key):
    try:
        d = json.load(open(f"{level_dir}/{name}"))
    except FileNotFoundError:
        return []
    return d if isinstance(d, list) else d.get(key, [])


stops = []
for t in sorted(load("triggers.json", "triggers"), key=lambda t: t["id"]):
    c, ax, sz = t["position"], t["axes"], t["size"]
    mid = [c[k] + sum(ax[a][k] * sz[a] / 2 for a in range(3)) for k in range(3)]
    mid[1] = c[1] + min(sz[1] / 2, 12)
    stops.append(("volume " + t["name"], mid, None))
for k in sorted(load("consoles.json", "consoles"), key=lambda k: k["id"]):
    m = k.get("marker")
    if m:
        stops.append((f"console {k['id']}", m["position"], m["facing"]))
lines = ["return {"]
for label, p, facing in stops:
    f = "nil" if facing is None else f"{facing:.4f}"
    lines.append(f'  {{"{label}", {p[0]:.2f}, {p[1]:.2f}, {p[2]:.2f}, {f}}},')
lines.append("}")
open(out, "w").write("\n".join(lines) + "\n")
