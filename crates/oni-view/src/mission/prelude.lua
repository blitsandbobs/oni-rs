-- The host side of the translated level scripts (`scripting/PORT.md`): the
-- tables and helpers every translated chunk receives. Engine access goes
-- through `__cmd`, which the engine provides while scripts run.

V, F, C, G = {}, {}, {}, {}

-- BSL values in Lua: numbers, strings, booleans (`true` / `false`).
local function num(x)
    if type(x) == "boolean" then return x and 1 or 0 end
    if type(x) == "string" then return tonumber(x) or x end
    return x
end

function T(x)
    if x == nil then return false end
    if type(x) == "boolean" then return x end
    if type(x) == "number" then return x ~= 0 end
    error("if needs an integer or boolean", 0)
end

function EQ(a, b)
    a, b = num(a), num(b)
    return a == b
end
function NE(a, b) return not EQ(a, b) end
local function cmp(a, b)
    a, b = num(a), num(b)
    if type(a) ~= type(b) then a, b = tostring(a), tostring(b) end
    return a, b
end
function LT(a, b) a, b = cmp(a, b) return a < b end
function GT(a, b) a, b = cmp(a, b) return a > b end
function LE(a, b) a, b = cmp(a, b) return a <= b end
function GE(a, b) a, b = cmp(a, b) return a >= b end
function ADD(a, b) return (tonumber(num(a)) or 0) + (tonumber(num(b)) or 0) end
function SUB(a, b) return (tonumber(num(a)) or 0) - (tonumber(num(b)) or 0) end

-- A bare word: an engine variable / value command, or else the word itself.
function W(name)
    local v = __cmd("@word", name)
    if v == nil then return name end
    return v
end

function SLEEP(n)
    local ticks = __cmd("@ticks", n)
    if ticks > 0 then coroutine.yield(ticks) end
end

-- Starts `f` as its own task right now: it runs until it first sleeps (the
-- scheduler then takes it over) before the caller carries on.
local function start(f, label, ...)
    local co = coroutine.create(f)
    local ok, n = coroutine.resume(co, ...)
    if not ok then return false, n end
    if coroutine.status(co) ~= "dead" then __cmd("@adopt", co, n, label) end
    return true
end

-- `fork f …`: an error in the fork aborts its parent too.
function FORK(f, ...)
    local label = "fork"
    for k, v in pairs(F) do
        if v == f then label = "fork " .. k break end
    end
    local ok, e = start(f, label, ...)
    if not ok then error(e, 0) end
end

-- A script function the engine runs at once (a spawn script, …).
function RUN(name, ...)
    local f = rawget(F, name)
    if not f then __cmd("@error", "no script function " .. name) return end
    local ok, e = start(f, name, ...)
    if not ok then __cmd("@error", name .. ": " .. tostring(e)) end
end

function ERR(where, what)
    __cmd("@error", where .. ": " .. what)
end

setmetatable(G, {
    __index = function(_, k) return __cmd("@get", k) end,
    __newindex = function(_, k, v) __cmd("@set", k, v) end,
})

-- Engine commands: each a call into the engine; a blocking one waits
-- (yields) until the engine says it is done.
setmetatable(C, {
    __index = function(t, k)
        local f = function(...)
            local r, wait, now, arg = __cmd(k, ...)
            if now then RUN(now, arg) end
            if type(wait) == "number" then
                -- Done in so many ticks: sleep that long.
                if wait > 0 then coroutine.yield(wait, k) end
            else
                -- Stalled: the same command is called again each tick until
                -- it stops stalling. (Yielding its name: what the task waits on.)
                while wait do
                    coroutine.yield(1, k)
                    r, wait = __cmd(k, ...)
                end
            end
            return r
        end
        rawset(t, k, f)
        return f
    end,
})

-- Calling a script function that doesn't exist: an error, not a crash.
setmetatable(F, {
    __index = function(_, k)
        return function() __cmd("@error", "no script function " .. k) end
    end,
})

-- Our own fixes to the shipped scripts (`docs/SCRIPT_FIXES.md`), run after
-- the level's chunk unless `--faithful`: `FIX(id, name, make)` replaces script
-- function `name` with `make(original)`. A function the scripts don't have is
-- an error (the fix has drifted from the scripts).
function FIX(id, name, make)
    local orig = rawget(F, name)
    if not orig then
        __cmd("@error", id .. ": no script function " .. name)
        return
    end
    rawset(F, name, make(orig))
end

-- Where a character stands (feet): x, y, z — the player when `who` is nil;
-- nothing if it isn't present. For the fixes only.
function CHR_AT(who)
    local p = __cmd("@chr_at", who)
    if p == nil then return nil end
    return p[1], p[2], p[3]
end
function PLAYER_AT() return CHR_AT() end

-- Runs a translated chunk.
function __load(chunk, name)
    local f, e = load(chunk, name)
    if not f then error(e) end
    f(V, F, C, G, W, T, EQ, NE, LT, GT, LE, GE, ADD, SUB, SLEEP, FORK, ERR)
end
