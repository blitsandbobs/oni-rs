#!/bin/sh
# Publishes our code to the public repository (blitsandbobs/oni-rs): the
# private repository's history, filtered — only oni-rs/, as the root; the
# working notes (CLAUDE.md) and the docs since deleted left out of every
# commit. The filter is the same every time, so the commits already public
# come out identical and a publish only adds the new ones (a fast-forward).
#
#   scripts/publish.sh            build the filtered history and show it
#   scripts/publish.sh --push     …and push it to the public main branch
#
# Needs git-filter-repo. Never touches the private repository.
set -eu
PUBLIC=${ONI_PUBLIC_REMOTE:-https://github.com/blitsandbobs/oni-rs.git}
root=$(cd "$(dirname "$0")/../.." && pwd)
work=${TMPDIR:-/tmp}/oni-publish
rm -rf "$work"
git clone --quiet --no-local "$root" "$work"
cd "$work"

# Left out of every commit: the working notes, and docs since deleted
# (fixed list — a filter that changed would rewrite what is public).
cat > ../oni-publish-drop.txt <<'EOF'
CLAUDE.md
ENGINE.md
ENGINE_NOTES.md
todo.md
docs/AI.md
docs/ASSET_FORMAT.md
docs/AUDIO.md
docs/CAMERA.md
docs/CHEATS.md
docs/COMBAT.md
docs/CORPSES.md
docs/DOORS.md
docs/ENGINE_NOTES.md
docs/ENGINE.md
docs/GAME_FLOW.md
docs/GOALS.md
docs/HUD.md
docs/LASERS.md
docs/MELEE_AI.md
docs/MENU.md
docs/MOVEMENT.md
docs/MOVIES.md
docs/NEUTRAL.md
docs/OBJECTS.md
docs/PARTICLES.md
docs/PATHFINDING.md
docs/PERCEPTION.md
docs/POWERUPS.md
docs/QUIRKS.md
docs/RENDERING.md
docs/SCRIPT_FIXES.md
docs/SKY.md
docs/SOUND_ENGINE.md
docs/sound/ORIGINAL_CORE.md
docs/sound/ORIGINAL_TRIGGERS.md
docs/TURRETS.md
docs/WEAPONS.md
EOF
git filter-repo --quiet --force --subdirectory-filter oni-rs
git filter-repo --quiet --force --invert-paths --paths-from-file ../oni-publish-drop.txt
rm ../oni-publish-drop.txt

echo "filtered: $(git rev-list --count HEAD) commits, HEAD $(git rev-parse --short HEAD)"
for f in CLAUDE.md docs/COMBAT.md; do
    if git log --all --format=%h -- "$f" | grep -q .; then
        echo "publish: $f is still in the history" >&2
        exit 1
    fi
done

if [ "${1:-}" = "--push" ]; then
    git push "$PUBLIC" HEAD:main
else
    echo "dry run: $work holds it; --push sends it to $PUBLIC (main)"
fi
