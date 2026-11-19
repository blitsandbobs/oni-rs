-- Gun sounds (docs/AUDIO.md, *Particles*): level 1's pistol — its firing
-- particle's ambient (the handgun's report) starts on the shot, its bullet
-- whines on the way (an ambient that follows it), the hit sounds by what it
-- struck (the dummy: a body; the far wall: the level's material), and an
-- empty gun clicks.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/gunfire.lua

step(20)
local px, py, pz = pos()
local d = spawn(px, py + 5, pz + 30, 180)
step(40)
facing(0)
step(5)
assert(give_weapon(0, "w1_tap"), "a pistol")
step(12) -- (the gun swings from the hand to the aim over 8 ticks)

local clips, ambients = {}, {}
local function listen(n)
    for _ = 1, n do
        step(1)
        for _, c in ipairs(channels()) do
            if c.clip then clips[c.clip] = true end
        end
        for _, s in ipairs(sounds()) do ambients[s.name] = true end
    end
end
local function any(pattern)
    for clip in pairs(clips) do
        if clip:match(pattern) then return clip end
    end
end

local hp0 = hp(d)
fire(1)
listen(20)
print("ambients:", (ambients.tctf_handgun and "tctf_handgun" or "-"), (ambients.ap_wiz and "ap_wiz" or "-"), "hit:", any("^ap_hit") or "-", "dummy hp", hp0, "→", hp(d))
assert(hp(d) < hp0, "the shot landed")
assert(ambients.tctf_handgun, "the handgun's report")
assert(ambients.ap_wiz, "the bullet's whine")
-- (A pistol round into cloth: the table gives the impact slot to the
-- pistol's own cloth entry, which makes no sound — as shipped.)

-- Fire at the room (over the dummy): the shots, their echoes, the hits on
-- the walls by material, ricochets, the casings landing.
clips = {}
for _ = 1, 4 do
    aim(0, 0.3)
    fire(1)
    listen(14)
end
local all = {}
for c in pairs(clips) do all[#all + 1] = c end
print("into the room:", table.concat(all, " "))
assert(any("^ap_hit_") or any("^ap_ric"), "the rounds hit the walls")
assert(any("^shell"), "the casings land")

-- Held down until it runs dry, the (automatic) pistol clicks. (A single
-- press on an empty gun with no clip punches instead — no click, as
-- shipped.)
clips = {}
aim(0, 0.3)
hold_fire(true)
listen(150)
hold_fire(false)
print("empty:", any("^weapon_empty") or "-")
assert(any("^weapon_empty"), "an empty gun clicks")
print("gunfire ok")
