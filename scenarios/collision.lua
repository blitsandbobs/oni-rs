-- Move-and-slide: running into a wall stops her (was the --walk test).
--
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/collision.lua

-- Face -X (toward the storage lockers) and run into them.
facing(-90)
input(true, false)
step(60)
local x, y, z = pos()
print(string.format("ran into the wall: feet %.0f,%.0f,%.0f  (started x=200)", x, y, z))

-- Open floor would carry her ~40 units; the lockers stop her well short, and
-- she must not tunnel through them.
assert(x < 198, "she moved toward the wall")
assert(x > 174, "the wall stopped her (no tunnelling), got x=" .. x)
assert(grounded(), "still on the ground")

cam(205, 12, 316,  186, 6, 300)
screenshot("/tmp/oni_collision.png", 760, 760)
