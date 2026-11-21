-- Level 12 (roof): our fixes to the shipped scripts (docs/SCRIPT_FIXES.md).

-- S5: Mukade can be stranded off the roof. Knocked off its open north end,
-- he lands on a lower roof, still above the height (550) at which his AI
-- brings him back. He can't climb back up. His teleport in behind his enemy
-- needs a roll that is kept until he next reaches melee, so with the roll
-- against him he never teleports, and the fight (the level's end) can't be
-- finished. Fixed, as the community's mods do: stranded off the roof for 3
-- seconds, he teleports back to its centre (flag 96).
local function on_roof(x, y, z)
    return y > 640 and x > 1580 and x < 1890 and z > -3330 and z < -2940
end

local function watch()
    local off = 0
    while V.__s5_watching == 1 do
        SLEEP(30)
        local x, y, z = CHR_AT("OutroNinja")
        if x == nil then
            off = 0
        elseif y >= 550 and not on_roof(x, y, z) then
            off = off + 30
            if off >= 180 then
                off = 0
                C.chr_animate("OutroNinja", "NINCOMteleport_in", 47)
                SLEEP(47)
                C.chr_teleport("OutroNinja", 96)
                C.chr_animate("OutroNinja", "NINCOMteleport_out", 31)
            end
        else
            off = 0
        end
    end
end

local function start_watching()
    if V.__s5_watching ~= 1 then
        V.__s5_watching = 1
        FORK(watch)
    end
end

-- The fight begins at the end of his entrance (`ninja`), or on restoring
-- save point 4 (`start`); it ends with `outro`.
FIX("S5", "ninja", function(ninja)
    return function(...)
        ninja(...)
        start_watching()
    end
end)
FIX("S5", "start", function(start)
    return function(...)
        start(...)
        if EQ(V.my_save_point, 4) then
            start_watching()
        end
    end
end)
FIX("S5", "outro", function(outro)
    return function(...)
        V.__s5_watching = 0
        return outro(...)
    end
end)
