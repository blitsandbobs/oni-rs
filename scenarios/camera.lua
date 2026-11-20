-- The game camera (docs/CAMERA.md): it follows the player from behind and
-- above along the aim, eased; looking up it sinks, looking down with a gun it
-- closes in; look mode (V) swings the view without turning the body, and the
-- turn drifts back when let go; walls between the camera and the player go
-- see-through ("jello").
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/camera.lua

local function cam()
    local x, y, z, vx, vy, vz, busy = camera()
    return { x = x, y = y, z = z, vx = vx, vy = vy, vz = vz, busy = busy }
end

step(30)
local px, py, pz = pos()
local c = cam()
print("see-through quads at the start (a partition beside her):", jello())
local back = math.sqrt((c.x - px) ^ 2 + (c.z - pz) ^ 2)
print(string.format("following: %.1f back, %.1f up; looking (%.2f %.2f %.2f)", back, c.y - py, c.vx, c.vy, c.vz))
assert(math.abs(back - 33) < 2, "33 back")
assert(math.abs(c.y - py - 23) < 2, "15 up, and an unarmed canter of 8")
assert(c.vz > 0.8 and c.vy < 0, "looking ahead (+z) and a little down")

-- Looking up, it sinks (the camera direction tips up, so it goes behind
-- and below); looking down, it rises.
aim(0, 0.6)
step(90)
local up = cam()
aim(0, -0.6)
step(90)
local down = cam()
print(string.format("looking up: %.1f high; down: %.1f high", up.y - py, down.y - py))
assert(up.y < c.y - 5 and down.y > c.y + 5, "it rises and sinks with the aim")
aim(0, 0)
step(90)

-- A turn: the camera eases round, it doesn't snap.
set_facing(0, 90)
step(1)
local turning = cam()
step(90)
local turned = cam()
print(string.format("turned: after a tick looking (%.2f, %.2f), after 90 (%.2f, %.2f)", turning.vx, turning.vz, turned.vx, turned.vz))
assert(turning.vx < 0.9 and turned.vx > 0.95, "it eases round")

-- Look mode: the mouse's turn goes to the aim, not the body; let go, it
-- drifts back 0.03 a tick.
look_mode(true)
aim_lr(0.6)
step(60)
local looking = cam()
look_mode(false)
step(10)
print(string.format("look mode: looking (%.2f, %.2f); let go, the turn is %.2f after 10 ticks", looking.vx, looking.vz, aim_lr()))
assert(math.abs(looking.vz) > 0.3 and looking.vx < 0.9, "the view swings with the look-mode turn")
assert(math.abs(aim_lr() - 0.3) < 0.01, "and drifts back when let go")
step(30)

-- In a narrow corridor the camera ends up behind its wall: the wall between
-- it and her goes see-through.
teleport(0, -75, 2, -500)
set_facing(0, 0)
step(150)
print("see-through quads in the corridor:", jello())
assert(jello() > 0, "the wall behind her is see-through")
-- ... and is drawn so: a frame's see-through triangles (the renderer finds
-- the jello's quads among its triangles — it once built that map before the
-- quads loaded, and drew every wall solid).
screenshot("/tmp/oni_camera_jello.png", 160, 120)
print("see-through triangles drawn:", jello_drawn())
assert(jello_drawn() > 0, "the see-through wall is drawn see-through")

-- A script camera: cm_detach holds it still; cm_reset follows again;
-- cm_interpolate eases to a camera clip's first frame, busy (cm_wait waits)
-- until it's there.
mission_begin(false)
step(5)
mission_exec([[
F.t_detach = function() C.cm_detach() end
F.t_reset = function() C.cm_reset() end
F.t_ease = function() C.cm_interpolate("KonCamFoot02", 60) C.cm_wait() V.eased = true end
]])
mission_call("t_detach")
step(3)
local held = cam()
teleport(0, -75, 2, -450)
step(30)
local still = cam()
print(string.format("detached: the camera moved %.2f", math.abs(held.z - still.z)))
assert(math.abs(held.z - still.z) < 0.01, "detached, it stays put")
mission_call("t_reset")
step(2)
local again = cam()
local qx, qy, qz = pos()
local gap = math.sqrt((again.x - qx) ^ 2 + (again.z - qz) ^ 2)
print(string.format("reset: %.1f from her", gap))
assert(math.abs(gap - 33) < 3, "reset, it is behind her again")
mission_call("t_ease")
step(30)
assert(cam().busy, "easing, it is busy")
local free
for t = 31, 90 do
    step(1)
    if not cam().busy then free = t break end
end
local eased = cam()
print(string.format("eased to the clip's start in %s ticks: (%.1f %.1f %.1f)", tostring(free), eased.x, eased.y, eased.z))
assert(free and free >= 58 and free <= 63, "60 ticks on, it is there and free")
print("ok")
