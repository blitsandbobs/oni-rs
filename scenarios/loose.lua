-- Loose things: a dropped gun falls with
-- gravity, meets the floor (its impact) and slides to rest; a dying
-- character drops its "death candy" — powerups that fall and come to rest
-- (their physics gone after 120 still ticks); a character a weapon lifts is
-- pulled up toward its lifter and falls when let go. Loose things meet
-- each other and characters too (`PHrPhysics_Single_PhyCollision`): a gun
-- dropped onto a lying gun comes to rest on it, one dropped onto a
-- character's head stops on him (or glances off), not falling through.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/loose.lua

step(20)
local x, y, z = pos()

-- A pistol let fall from 30 up: down in a few ticks (gravity 0.294 a
-- tick), its impact on the floor, then still.
local before_guns = #loose_guns()
assert(spawn_weapon("w1_tap", x + 20, y + 30, z, true))
local landed, hit = nil, false
for k = 1, 90 do
    step(1)
    local g = loose_guns()[before_guns + 1]
    for _, c in ipairs(channels()) do
        if c.clip and c.clip:match("hit") then hit = true end
    end
    if not landed and g.at[2] < y + 2 then landed = k end
end
local g = loose_guns()[before_guns + 1]
print(string.format("the pistol: down after %s ticks, at y %.2f, moving %s, impact %s", tostring(landed), g.at[2], tostring(g.moving), tostring(hit)))
assert(landed and landed < 20 and not g.moving and hit, "falls fast, lands with its impact, lies still")

-- Bay2_Thug_1 drops a hypo dying.
local t = place("Bay2_Thug_1")
step(5)
local before = #powerups()
local _, ty = pos_of(t)
set_hp(t, 0)
damage(t, 1)
local dropped
for k = 1, 120 do
    step(1)
    if #powerups() > before then dropped = k break end
end
print("the thug drops", #powerups() - before, "after", dropped)
assert(dropped, "death candy")
local p = powerups()[#powerups()]
print(string.format("a %s at (%.1f %.1f %.1f), falling %s", p.kind, p.at[1], p.at[2], p.at[3], tostring(p.falling)))
assert(p.kind == "hypo" and p.falling, "a hypo, falling")
step(200)
p = powerups()[#powerups()]
print(string.format("after: at y %.1f, falling %s", p.at[2], tostring(p.falling)))
assert(not p.falling and p.at[2] < ty + 4, "come to rest on the floor")

-- Konoko lifts a Striker 50 away for a second: up toward a glide path above
-- her, then it falls.
local s = spawn(x, y + 5, z + 50, 180, "striker_easy_1")
step(20)
local _, sy0 = pos_of(s)
lift(s, 0, 60)
local top = sy0
for _ = 1, 60 do
    step(1)
    local _, sy = pos_of(s)
    top = math.max(top, sy)
end
print(string.format("lifted: from y %.1f up to %.1f", sy0, top))
assert(top > sy0 + 5, "pulled up")
step(120)
local _, sy = pos_of(s)
print(string.format("let go: back to y %.1f", sy))
assert(sy < top - 3, "it falls when let go")

-- A second pistol let fall on the first: it rests on it, not through it.
local first = loose_guns()[before_guns + 1]
assert(spawn_weapon("w1_tap", first.at[1], first.at[2] + 30, first.at[3], true))
step(120)
local second = loose_guns()[#loose_guns()]
print(string.format("on the pistol: at y %.2f (the first at %.2f)", second.at[2], first.at[2]))
assert(second.at[2] > first.at[2] + 1, "rests on the first")

-- One let fall on a standing thug's head: it stops there, not through him.
local t2 = spawn(x - 40, y + 5, z, 0, "thug_wh_1")
step(30)
local tx, tyy, tz = pos_of(t2)
assert(spawn_weapon("w1_tap", tx + 1, tyy + 40, tz, true))
step(120)
local third = loose_guns()[#loose_guns()]
local off = math.sqrt((third.at[1] - tx) ^ 2 + (third.at[3] - tz) ^ 2)
print(string.format("on the thug: %.1f off him, at y %.2f", off, third.at[2]))
assert(off > 4 or third.at[2] > tyy + 10, "not through him")
print("loose ok")
