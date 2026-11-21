-- Loose things (docs/OBJECTS.md, *Loose things*): a dropped gun falls with
-- gravity, meets the floor (its impact) and slides to rest; a dying
-- character drops its "death candy" — powerups that fall and come to rest
-- (their physics gone after 120 still ticks); a character a weapon lifts is
-- pulled up toward its lifter and falls when let go.
--
--   cargo run --release -p oni-view -- \
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
print("loose ok")
