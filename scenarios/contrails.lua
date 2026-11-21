-- Contrails (#73; docs/PARTICLES.md, *Contrails*): level 9's generators
-- crackle with electric arcs, contrail particles joined link by link into
-- bands. Each live link draws one band.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/09/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/contrails.lua

mission_begin(true, 1)
for _ = 1, 240 do
  pcall(skip_cutscene)
  step(10)
  if player_ready() then break end
end
step(300)
local arcs = #particles_of("env_elec_p02")
local bands = world_stats().contrails
print("arc particles:", arcs, "bands drawn:", bands)
assert(arcs > 20, "the generators' arcs run")
assert(bands > 50, "and their links are drawn as bands")
print("ok")
