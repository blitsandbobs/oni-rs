-- oni_api.lua — SKELETON of the gameplay-scripting surface (BSL → Lua port).
--
-- Groundwork only: this pins down the SHAPE of the host API before any engine
-- subsystem is wired. Every function is a stub that raises `not implemented`.
-- It is NOT loaded by the engine; it documents what the host (Rust) must expose
-- and how a translated `.bsl` mission script would call it. See PORT.md / BSL.md.
--
-- Backing: each stub is eventually a Rust host function bound into the Lua state
-- (mlua), driven by the engine subsystem named in its comment. Entity args are
-- names (string) or ids (int) — never handles (BSL convention).

local function todo(name)
  return function(...)
    error("oni_api: '" .. name .. "' not implemented (groundwork stub)", 2)
  end
end

local oni = {}

--------------------------------------------------------------------------------
-- Scheduling / cooperative control (host scheduler + Lua coroutines).
-- Blocking host calls yield; the per-tick pump resumes them when the engine
-- signals completion. `schedule`/`fork` run funcs as managed coroutines.
--------------------------------------------------------------------------------
oni.wait      = todo("wait")       -- wait(ticks): yield the current script N ticks (BSL `sleep`)
oni.schedule  = todo("schedule")   -- schedule(fn, every, repeat_count): periodic coroutine
oni.fork      = todo("fork")       -- fork(fn, ...): run fn as an independent task
oni.win       = todo("win")        -- end the level: success   (Oni_GameState.c)
oni.lose      = todo("lose")       -- end the level: failure

--------------------------------------------------------------------------------
-- Mission / UI orchestration — LAYER 1 (pure, build first). Oni_InGameUI.c.
--------------------------------------------------------------------------------
oni.objective = {
  new      = todo("objective.new"),       -- objective.new(id)
  set      = todo("objective.set"),       -- objective.set(id)
  complete = todo("objective.complete"),  -- objective.complete(id)
  prompt   = todo("objective.prompt"),
}
oni.message       = todo("message")        -- message(id): subtitle/HUD text
oni.splash_screen = todo("splash_screen")
oni.timer = {
  start = todo("timer.start"),  -- timer.start(id, ticks, on_expire_fn)
  stop  = todo("timer.stop"),
}
oni.save = {
  game  = todo("save.game"),
  point = todo("save.point"),
}

--------------------------------------------------------------------------------
-- Objects / triggers — LAYER 2 (light). Oni_Object/*, OT_Door.c, OT_Turret.c.
--------------------------------------------------------------------------------
oni.door = {
  open   = todo("door.open"),    -- door.open(door_id)
  close  = todo("door.close"),
  lock   = todo("door.lock"),
  unlock = todo("door.unlock"),
}
oni.obj = {
  create = todo("obj.create"),
  kill   = todo("obj.kill"),
  show   = todo("obj.show"),     -- obj.show(obj_id, on)
}
oni.env = {
  anim    = todo("env.anim"),    -- env.anim(obj_id, anim) [blocking variant: env.anim_block]
  show    = todo("env.show"),
  texswap = todo("env.texswap"),
}
oni.console = {
  activate   = todo("console.activate"),   -- console.activate(console_id)
  deactivate = todo("console.deactivate"),
  locked     = todo("console.locked"),
}
oni.turret = {
  activate   = todo("turret.activate"),
  deactivate = todo("turret.deactivate"),
}

--------------------------------------------------------------------------------
-- Camera / cutscene — LAYER 3 (medium; blocking). Oni_Camera.c, Oni_Cinematics.c.
-- The *_block forms yield until the move/cut completes.
--------------------------------------------------------------------------------
oni.cm = {
  anim        = todo("cm.anim"),         -- cm.anim(name)  / cm.anim_block(name)
  orbit       = todo("cm.orbit"),
  interpolate = todo("cm.interpolate"),
  detach      = todo("cm.detach"),
  reset       = todo("cm.reset"),
  wait        = todo("cm.wait"),         -- yield until the camera settles
}
oni.cinematic = {
  start        = todo("cinematic.start"),
  stop         = todo("cinematic.stop"),
  letterbox    = todo("cinematic.letterbox"),
  fade_in      = todo("cinematic.fade_in"),
  fade_out     = todo("cinematic.fade_out"),
  cutscene_sync = todo("cinematic.cutscene_sync"),
}
oni.sound = {
  music_start = todo("sound.music_start"),   -- sound.music_start(name)
  music_stop  = todo("sound.music_stop"),
  ambient     = todo("sound.ambient"),
  dialog_play = todo("sound.dialog_play"),   -- dialog_play_block yields until the line ends
  impulse     = todo("sound.impulse"),
}

--------------------------------------------------------------------------------
-- Characters — LAYER 4 (deep; needs the character SM + animation + inventory).
-- ONI_Character.c. `wait_*` / `animate_block` yield until the action finishes.
--------------------------------------------------------------------------------
oni.chr = {
  create     = todo("chr.create"),      -- chr.create(from_file)
  teleport   = todo("chr.teleport"),    -- chr.teleport(who, flag_id)
  delete     = todo("chr.delete"),
  animate    = todo("chr.animate"),     -- chr.animate(who, anim) [blocking: chr.animate_block]
  playback   = todo("chr.playback"),
  health     = todo("chr.health"),      -- getter → int
  set_health = todo("chr.set_health"),
  invincible = todo("chr.invincible"),
  giveweapon = todo("chr.giveweapon"),
  disarm     = todo("chr.disarm"),
  facetoflag = todo("chr.facetoflag"),
  talk       = todo("chr.talk"),        -- play a dialogue line
  vocalize   = todo("chr.vocalize"),
  changeteam = todo("chr.changeteam"),
  is_player  = todo("chr.is_player"),   -- → bool
  wait_animation = todo("chr.wait_animation"),  -- yield until `who` finishes its anim
}

--------------------------------------------------------------------------------
-- AI — LAYER 5 (deepest; needs the whole AI2 system). Oni_AI2_Script.c.
--------------------------------------------------------------------------------
oni.ai2 = {
  active        = todo("ai2.active"),     -- ai2.active(who)
  inactive      = todo("ai2.inactive"),
  passive       = todo("ai2.passive"),
  attack        = todo("ai2.attack"),     -- ai2.attack(who, target)
  comehere      = todo("ai2.comehere"),
  movetoflag    = todo("ai2.movetoflag"),
  dopath        = todo("ai2.dopath"),
  doalarm       = todo("ai2.doalarm"),
  makeblind     = todo("ai2.makeblind"),
  makedeaf      = todo("ai2.makedeaf"),
  ignore_player = todo("ai2.ignore_player"),
  spawn         = todo("ai2.spawn"),
  forget        = todo("ai2.forget"),
}
oni.difficulty      = todo("difficulty")         -- → int
oni.did_kill        = todo("did_kill")           -- scripted-boss flags, e.g. did_kill("griffen")
oni.trigvolume_count = todo("trigvolume_count")  -- trigvolume_count(trig_id) → int

--------------------------------------------------------------------------------
-- Event handlers. A mission script defines these; the host dispatches them with
-- the entity name, matching BSL's spawn_/die_/combat_/alarm_ handler table and
-- trigger entry/inside/exit scripts.
--------------------------------------------------------------------------------
oni.on = {
  level_begin   = todo("on.level_begin"),   -- the BSL `main`
  char_spawn    = todo("on.char_spawn"),    -- on.char_spawn(name, fn)   fn(self)
  char_die      = todo("on.char_die"),
  char_combat   = todo("on.char_combat"),
  char_alarm    = todo("on.char_alarm"),
  trigger_enter = todo("on.trigger_enter"), -- on.trigger_enter(trig_id, fn)  fn(self, volume)
  trigger_exit  = todo("on.trigger_exit"),
  timer_expire  = todo("on.timer_expire"),
}

return oni

-- Example of a translated mission fragment (illustrative; not executed here):
--
--   local oni = require("oni_api")
--   function main()
--     oni.cm.reset()
--     oni.objective.new(1)
--     oni.sound.music_start("neutral")
--     oni.schedule(check_mission, 60, 0)      -- every 60 ticks, forever
--   end
--   function check_mission(self)
--     if oni.trigvolume_count(3) == 0 then oni.win() end
--   end
--   oni.on.char_die("griffen", function(self) oni.message(42) end)
