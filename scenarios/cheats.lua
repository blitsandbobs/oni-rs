-- The original's cheat codes that change the game (#90), typed on the pause
-- screen once allowed: `bighead`, `minime`, `behemoth`, `fistsoflegend`,
-- `roughjustice`, `munitionfrenzy`, `reservoirdogs`.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/cheats.lua

step(20)
local px, py, pz = pos()
cheats(true)

local function cheat(code)
    pause_toggle()
    local message = type_keys(code)
    pause_press("close")
    return message
end

local function length(v)
    return math.sqrt(v[1] ^ 2 + v[2] ^ 2 + v[3] ^ 2)
end

-- bighead: every head posed 4 times its size.
local head = length(bone(0, 10).x)
assert(cheat("bighead") == "Big Head Enabled")
step(1)
local big = length(bone(0, 10).x)
print(string.format("head axis %.2f -> %.2f", head, big))
assert(math.abs(big / head - 4) < 0.01, "a head 4 times the size")
assert(cheat("bighead") == "Big Head Disabled")
step(1)
assert(math.abs(length(bone(0, 10).x) - head) < 0.01, "back to its size")

-- minime: a quarter the size, and back.
assert(cheat("minime") == "Mini Mode Enabled")
assert(math.abs(scale_of(0) - 0.25) < 1e-4, "a quarter the size")
assert(cheat("minime") == "Mini Mode Disabled")
assert(math.abs(scale_of(0) - 1) < 1e-4, "full size")

-- behemoth: 1.8 times the size; off, a size from her class's range.
assert(cheat("behemoth") == "Godzilla Mode Enabled")
assert(math.abs(scale_of(0) - 1.8) < 1e-4, "Godzilla")
assert(cheat("behemoth") == "Godzilla Mode Disabled")
assert(scale_of(0) < 1.5, "back to her class's size, got " .. scale_of(0))
step(20)

-- fistsoflegend: a punch blows the dummy up.
local dummy = spawn(px, py + 5, pz + 6, 0)
step(60)
assert(cheat("fistsoflegend") == "Fists Of Legend Enabled")
facing(0)
punch()
local blown = false
for _ = 1, 40 do
    step(1)
    local a = anim_of(dummy)
    if a:find("blownup") or a:find("BLOWNUP") then blown = true end
end
print("dummy after the punch:", anim_of(dummy))
assert(blown, "the punch blew the dummy up")
assert(cheat("fistsoflegend") == "Fists Of Legend Disabled")

-- roughjustice: the player's guns chamber in their cheat times (the
-- Campbell Equalizer: 4 ticks a shot, not 12).
local function shots_in(ticks)
    give_weapon(0, "w1_tap")
    step(5)
    local _, ammo = weapon(0)
    hold_fire(true)
    step(ticks)
    hold_fire(false)
    local _, left = weapon(0)
    step(30)
    return ammo - left
end
local plain = shots_in(48)
assert(cheat("roughjustice") == "Gatling Guns Enabled")
local gatling = shots_in(48)
print("shots in 48 ticks:", plain, "->", gatling)
assert(plain >= 3 and plain <= 5, "every 12 ticks, got " .. plain)
assert(gatling >= 2 * plain, "every 4 ticks, got " .. gatling)
assert(cheat("roughjustice") == "Gatling Guns Disabled")

-- munitionfrenzy: one gun of every class round the player.
local lying = weapons_lying()
assert(cheat("munitionfrenzy") == "Weapons Locker Created")
assert(weapons_lying() == lying + 12, "a gun of each of the 12 classes")

-- reservoirdogs: everyone hostile to everyone — two Strikers on Konoko's
-- team fight.
local a = spawn(px + 40, py + 5, pz, 90, "striker_easy_1")
local b = spawn(px + 60, py + 5, pz, 270, "striker_easy_1")
team(dummy, "konoko")
for _, s in ipairs({ a, b }) do
    team(s, "konoko")
    goals(s)
    watch(s)
end
step(60)
assert(not goal_of(a):find("combat") and not goal_of(b):find("combat"), "friends don't fight")
assert(cheat("reservoirdogs") == "Last Man Standing Enabled")
local fought = false
for _ = 1, 120 do
    step(1)
    if goal_of(a):find("combat") or goal_of(b):find("combat") then fought = true end
end
print("striker:", goal_of(a))
assert(fought, "last man standing: the Strikers fight")
assert(cheat("reservoirdogs") == "Last Man Standing Disabled")
print("cheats ok")
