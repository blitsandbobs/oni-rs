# Scenarios

Headless Lua scripts that drive the real engine and assert — the project's
tests of behaviour. Each file's header comment says what it checks and how
to run it; `scenarios/run_all.sh [--faithful]` runs them all, and
`scripts/check.sh` runs every one in both modes as part of the gate.

How scenarios work: `docs/lua.md`. Every function they can call:
`docs/lua-api.md`.
