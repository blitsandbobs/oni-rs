-- Other character classes: each spawns with its own body, skin, animation
-- set and hit points, stands in its own idle, takes hits, and can be thrown —
-- a victim plays the thrower's side of the throw even from another set.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/classes.lua

local STAND, THROW_FORWARD_PUNCH = 6, 106
local function thrown(t) return (t >= 96 and t <= 101) or (t >= 114 and t <= 119) or (t >= 128 and t <= 132) end

step(20)
local px, py, pz = pos()
set_facing(0, 0)

-- A line-up across the room, all facing Konoko.
local lineup = { "striker_easy_1", "striker_hard_1", "ninja_med_1", "thug_mfg_1", "tanker_med_1", "elite_med_1" }
local ids = {}
for i, class in ipairs(lineup) do
    ids[i] = spawn(px - 25 + 10 * (i - 1), py + 5, pz + 40, 180, class)
end
step(90)
for i, id in ipairs(ids) do
    print(string.format("%-16s id %d: class %s, hp %d, type %d (%s)", lineup[i], id, class_of(id), hp(id), type_of(id), anim_of(id)))
    assert(class_of(id) == lineup[i], "spawned as asked")
    assert(hp(id) > 0, "has hit points")
    assert(anim_of(id) ~= nil and type_of(id) ~= 0, "plays a clip from its own set")
end
cam(px, py + 20, pz + 5, px, py + 6, pz + 40)
screenshot("/tmp/oni_classes.png", 1000, 500)

-- Konoko throws a Striker: the Striker plays Konoko's victim clip.
local s = spawn(px, py + 5, pz + 8, 180, "striker_easy_1")
step(60)
set_facing(0, 0)
local hp0 = hp(s)
input(true, false); punch(); step(1); input(false, false)
print(string.format("forward punch at a Striker: me %d (%s), striker %d (%s)", anim_type(), anim(), type_of(s), anim_of(s)))
assert(anim_type() == THROW_FORWARD_PUNCH, "Konoko throws the Striker")
assert(thrown(type_of(s)) and anim_of(s):find("^KON"), "the Striker plays Konoko's victim clip")
for _ = 1, 160 do step(1) end
print(string.format("striker after the throw: hp %d -> %d, state %d (%s)", hp0, hp(s), state_of(s), anim_of(s)))
assert(hp(s) < hp0, "the throw hurt the Striker")
for _ = 1, 300 do step(1) end
print(string.format("striker later: type %d (%s)", type_of(s), anim_of(s)))
assert(not anim_of(s):find("^KON"), "back on its own clips afterwards")
print("classes ok: other classes spawn with their own sets, skins and hit points; cross-class throws work")
