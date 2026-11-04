# BeamAssist (formerly BeamNG.Speak) — progress notes

Last updated: 2026-11-03

## Goal

Voice-control assistant for BeamNG.drive.

- Local, lightweight models only.
- Always-listening speech recognition, gated by a hotword and/or phrasing.
- A Jev-style classifier/router picks the intent.
- Responses are in-game messages only (no TTS).
- Talks to the game's embedded MCP server. It can run on another LAN machine; the user handles TCP port proxying.
- Ships as a single Windows exe. Only `config.toml` and `models/` sit beside it.
- The exe auto-downloads models and writes a default config if one is missing.
- The config allows alternative models and sets MCP host/port (default 127.0.0.1).

## Agreed design decisions

### Stack

- Rust, statically linked (`x86_64-pc-windows-msvc`, `crt-static`).
- Libraries:
  - `whisper-rs` for speech-to-text, with the built-in Silero voice activity detection.
  - llama.cpp bindings for the embedding model and a small LLM.
  - `cpal` for audio capture.
  - `rmcp` as the MCP client.
  - `reqwest` + `rustls` for downloads.
- Jev itself isn't bundled (jevos ships with OpenVINO DLLs). Instead the choice/confidence pattern is implemented in-process, behind a `Router` trait, with an optional `jev-http` backend.

### Pipeline

1. Microphone → voice activity detection → Whisper (vocabulary prompt includes vehicle and map names).
2. Gate: hotword (fuzzy match) and/or the router's `none` class falling below a threshold.
3. Router:
   - Embedding nearest-neighbour over example phrases.
   - Small LLM with a GBNF grammar used only for ties and slot filling.
   - The LLM outputs only `{intent, slots}`. It never writes Lua.
4. Resolver: maps the intent to Lua based on the current vehicle (see below).
5. `show_message` reports "intent — result".

### Stateless commands

- No caching. Each command is self-contained.
- The resolver library is written in Lua and embedded in the exe with `include_str!`. It is prepended to every chunk: probe → classify → act → verify, all within one vehicle-VM execution.
- Game-engine intents (spawn, map, camera, navigation) use short chains of MCP calls with captured variables.

### Abstract capabilities

- Intents target capabilities, e.g. `diff_lock {scope: rear, state: locked}` or `range {low}`. They never target specific devices.
- Scope expansion: "rear diff" means all rear diffs plus the inter-axle lock.

### Result contract (Lua → exe)

```json
{"status":"applied|already|partial|unsupported|failed","targets":[{"role":"...","from":"...","to":"..."}],"detail":null}
```

The exe composes the message from config templates:

- `[messages] format = "{intent} — {result}"`
- Per-intent labels, e.g. "Requested high beams".
- Per-status results, e.g. "already high".

Error cases:

- No response from the vehicle → "{intent} — no response from vehicle".
- Low confidence → optional "Heard '…', not understood".
- `show_message` fails → log to the console.

## MCP server facts (verified 2026-11-03)

- Endpoint `http://127.0.0.1:29292/mcp`: streamable HTTP, plain JSON responses.
- Server `beamng-game` v1.0.0, protocol `2025-06-18`. No `mcp-session-id`.
- About 100 tools.

Key tools:

| Tool | Notes |
|---|---|
| `run_lua` | Game-engine VM. Synchronous; returns `tostring()` of the result. |
| `run_lua_vehicle` | Vehicle VM. **Asynchronous**: each call returns the *previous* run's results as `{vehicleId: result}`. Args: `code`, `id`, `all`. |
| `show_message` | `text` (required), `icon` (Material icon name, default `info`), `ttl` (seconds, default 5), `category` (same category replaces the previous message; default `mcp`). **Tested OK.** Note the name is `show_message`, not `send_message`. |
| `get_electrics` | Async; `keys=[...]`. |
| `inject_input` | Input events: parkingbrake, clutch, etc. |
| `trigger_action` | Bound actions: recover_vehicle, toggleCamera, etc. |
| `get_status` | Level, vehicle, camera, sim state. |
| `get_vehicle`, `get_vehicles`, `get_part_tree`, `get_vehicle_damage` | |
| `spawn` | `model`, `config`, `add`. |
| `get_configs` | `model`. |
| `list_levels`, `load_level` | |
| `find_objects` | `class` (e.g. `SpawnSphere`), `pattern`, `name`. |
| `view_objects` | |
| `get_navgraph` | |
| `drive_to` | `pos`, `waypoint` or `targetId`; plus speed and aggression. |
| `set_ai` | |
| `set_camera`, `set_time_of_day`, `set_simulation_speed`, `recover_vehicle`, `reset_vehicle` | |

Destructive tools to exclude from the router allowlist: `delete_file`, `write_file`, `delete_object`, `load_level` (confirm first), `editor_save_level`, `reload_lua`, `remove_vehicle`, `unload_extension`.

### Reliable async pattern for `run_lua_vehicle`

1. Wrap the code: `local ok,r=pcall(function() <code> end) return '<TAG>'..jsonEncode({ok=ok,r=r})`
2. Call with that wrapped code.
3. Poll with `run_lua_vehicle {code:'return "noop"'}` every ~250 ms until a value starting with `<TAG>` appears.

This takes ~0.3–1 s per command.

### Model keys (useful for the vocabulary and the spawn intent)

`pigeon` = Ibishu Pigeon · `sunburst2` = Hirochi Sunburst · `pickup` = Gavril D-Series · `md_series` = Gavril MD-Series · `percheron` = Cherrier Percheron · `legran` = Bruckell LeGran · `bastion` = Bruckell Bastion

- Build the full list at runtime: `run_lua` → `core_vehicles.getModelList().models` (Brand + Name).
- Configs come from `get_configs`.

## Vehicle probe findings (verified 2026-11-03)

Configs tested: `pigeon/base`, `sunburst2/sport_RS_DCT`, `pickup/d15_4wd_M`, `percheron/s450_8x8_flatbed_A`.

### Powertrain

- `powertrain.getDevices()` gives devices by name. Each device has:
  - `type`, `uiName`, `mode`, `availableModes` (array), `defaultMode`
  - `parent`, `children`
  - `connectedWheel` (on wheel shafts)
  - `setMode` (only on mode-capable devices)
- **A device is switchable if `#availableModes > 1`.**
- Downstream wheels: walk `children` and collect `connectedWheel`. Wheel names vary (`FL`, `FL_2`, `R1LL`, `R2RR`), so classify by the F/R prefix.
- Observed device types:
  - `combustionEngine`, `frictionClutch`, `torqueConverter`
  - `manualGearbox`, `automaticGearbox`, `dctGearbox`
  - `rangeBox`, `differential` (also used for transfer cases and the inter-axle diff), `shaft`, `torsionReactor`
  - `hydrodynamicRetarder`, `compressor`, `hydraulicPump`, `electricMotor` (Percheron hydraulics)

| Vehicle | Switchable devices |
|---|---|
| Pigeon base | None. Single-mode RWD; `wheelaxleF` is disconnected (three-wheeler). |
| Sunburst RS DCT | None at device level. All diffs are `lsd` single-mode. Control is via controllers. |
| D-Series 4x4 M | `rangebox` (high/low), `transfercase_F` (connected/disconnected), `wheelaxleFL`/`FR` (hub lockouts). `transfercase` is locked single-mode. |
| Percheron 8x8 A | `tCaseRangebox` (high/low), `transfercase_F` (front drive), `transfercase_FF` (front inter-axle), `transfercase_R` (rear inter-axle), `differential_F`, `differential_F_2`, `differential_R_1`, `differential_R_2` (each open/locked). |

### Prefer `driveModes` controllers — they bundle devices correctly

- Enumerate: `controller.getAllControllers()` where `c.typeName=='driveModes'`.
- Mode keys: `v.data[ctrlName].modes`.
- Contents of one mode: `c.getDriveModeData(key).settings`. Each setting has a `type`:
  - `powertrainDeviceMode` (`deviceName`, `mode`)
  - `electricsValue` (`electricsName`, `value`)
  - `controller` (`activeCenterDiffLock`, `tractionControl`, …)
  - `transmission`
  - `simpleControlButton`, `quickAccess`
- Current mode: `c.getCurrentDriveModeKey()`. Set: `c.setDriveMode(key)`. **Verified working; also updates dash electrics.**

| Vehicle | Controller | Modes → what each sets |
|---|---|---|
| Percheron | `rearLockControl` | unlocked/locked → `differential_R_1`, `differential_R_2`, `transfercase_R`, `e.rear_lock` |
| | `frontLockControl` | unlocked/locked → `differential_F`, `differential_F_2`, `transfercase_FF`, `e.front_lock` |
| | `transfercaseControl` | unlocked/locked → `transfercase_F`, `e.front_drive`, `e.transfercase_lock` |
| | `rangeboxControl` | high/low → `tCaseRangebox`, `e.rangebox_state` |
| D-Series 4x4 | `transfercaseControl` | `2hi`/`4hi`/`4lo` → `rangebox`, `transfercase_F`, hub lockouts, `e.transfercase_state` (no 2-low) |
| Sunburst RS | `driveModes` | comfort/sport/off. `off` = "ESC & TC Off": `tractionControl` and `yawControl` read back `getConfig().isEnabled=false`. **Verified.** |
| | `centerDiffModes` | auto/lock0/lock50/lock100 (`activeCenterDiffLock`) |

### Resolver rules

1. Find `driveModes` controllers whose mode settings touch the target devices. Pick the mode that puts all of them in the requested state, then call `setDriveMode`.
2. Fallback: `device:setMode(m)`. Verified on `differential_R_1`, but it does not update the dash electrics.
3. Verify by reading `device.mode` back.

Pitfall: the legacy `4wd` controller's `setRangeMode('low')` is a silent no-op. Don't use it.

### Gearbox

- Setting `gearbox:setMode()` directly is overridden by the vehicle controller (the gear stays N).
- Use `controller.mainController.shiftToGearIndex(i)`. On the Sunburst DCT ("realistic" behaviour): `-1=R`, `0=N`, `1=P`, `2=D`, `3=S`.
- Verify via `electrics.values.gear`. The mapping may vary by gearbox type, so make it data-driven and verify-and-retry.
- `controller.mainController.getState().grb_mde` gives the current mode letter.

### Electrics

- There is no `electrics.state`. Read `electrics.values` (~130 keys, e.g. `watertemp`, `oiltemp`, `fuel`, `rpm`, `lights_state`, `highbeam`, `fog`, `esc`, `tcs`, `hasESC`, `hasTCS`, `ignitionLevel`, `gear`, `parkingbrake`).
- Functions available:
  - `setLightsState(0/1/2)`, `set_fog_lights`, `set_warn_signal`, `set_left_signal`, `set_right_signal`, `set_lightbar_signal`
  - `setIgnitionLevel`, `setAllowedIgnitionLevels`
  - `toggle_*` variants, including `toggle_highbeams`
- Verified working: `setLightsState(2)` (gives `highbeam=1`), `set_fog_lights(1/0)`, `set_warn_signal(1/0)`.
- **Signals are unreliable**: `set_left_signal(1)` was a no-op and `set_right_signal(0)` didn't turn the signal off. Use read-then-toggle: compare `signal_left_input`/`signal_right_input` with the target, then call `toggle_left_signal`/`toggle_right_signal`.
- Other controllers worth exposing:
  - Percheron: `airSuspension`, `compressionBrake`, `retarderControl`, `hydraulicsControl`.
  - Sunburst: `twoStepLaunch`.
  - All vehicles: door, tailgate and hood `advancedCouplerControl` couplers.

## Resolver library — `lua/resolver.lua` (v0.1, tested live 2026-11-03)

How the exe uses it:

- Each command sends the resolver source plus one line, `return BNGS.call(action, args, ruleOverrides, tag)`, via `run_lua_vehicle`.
- The exe then polls for the tag.
- The result is `tag .. jsonEncode(result)`.
- If `status == "pending"`, the exe waits ~400 ms and sends `verify {checks, action}`, which returns the final status. Electrics and gear changes only show up on the next frame, which is why they need this second pass.

### Actions

| Action | Args | Mechanism |
|---|---|---|
| `diff_lock` | `scope` = front/rear/center/inter/axles/all, `state` = locked/open | Device classification + scope table; `driveModes` controller planner, falling back to `setDeviceMode`; falls back to a centre-diff controller (Sunburst) |
| `range` | `state` = low/high | Same planner, using the `rangeBox` role |
| `drive` | `state` = 4wd/2wd | Same planner, using the `drive` + `hub` roles |
| `lights` | `state` = off/low/high | `setLightsState`, deferred verify |
| `fog`, `hazard` | `state` = on/off | Setters, deferred verify |
| `signal` | `side` = left/right/off | Read then toggle, deferred verify |
| `traction` | `state` = on/off | Picks a `driveModes` mode containing the `tractionControl` setting ("off" by key/name pattern); verifies `getConfig().isEnabled` |
| `gear` | `state` = P/R/N/D/S or a number | `shiftToGearIndex` using a per-gearbox-type map; manual gears are numeric |
| `read` | `keys` = [...] or `group` = temps/fuel/engine/speed/lights/assist | `electrics.values` |
| `list` | — | Inventory: switchable devices with roles, `driveModes` controllers and their modes, couplers, light electrics, assists, gearbox, motors |
| `verify` | `checks`, `action` | Deferred checks |

### Device classification (generic, no per-vehicle tables)

A device counts as switchable when it has two or more `availableModes`. Its role:

- `range`: type `rangeBox`.
- `diff`: a lock-capable `differential`. Its class is set by the wheels downstream (F/R prefix) and by how many diffs sit below it:
  - `center`: drives both front and rear wheels.
  - `inter_front` / `inter_rear`: has two or more diffs below it.
  - `diff_front` / `diff_rear`: otherwise.
- `drive`: a connected/disconnected shaft feeding several wheels.
- `hub`: a connected/disconnected shaft feeding one wheel.

### Planner

1. Work out the desired mode for each target device.
2. Pick `driveModes` controller modes that set targets correctly and never set a target wrong. Prefer the mode with the fewest side effects on non-target devices.
3. Send any devices not covered straight to `powertrain.setDeviceMode`.
4. Read every device back. Report the targets, plus any `side` changes (e.g. the D-Series has no 2-low, so "low range" also engages 4WD and reports that).

### Test results

| Vehicle | Results |
|---|---|
| Sunburst RS DCT | TC off/on via `driveModes` ✔, "already" ✔, centre lock100/auto ✔, rear lock → unsupported (LSD only) ✔, lights high/off/already ✔, signals left/right/off ✔, gear D/R/P ✔, read temps ✔, range → unsupported ✔ |
| Percheron 8x8 | Rear lock = R1 + R2 + inter-axle ✔, already ✔, all lock/open (6 devices) ✔, 4wd/2wd ✔, low/high ✔, gear D/N ✔ |
| D-Series 4x4 M | Low → `4lo` with side effects reported ✔, high stays 4WD (fewest side effects) ✔, 2wd (transfer case + hubs) ✔, rear lock and TC → unsupported ✔, manual gears 3/2/N/R ✔ |

The bug where manual gears read as numbers was fixed (`gearName`).

Known behaviour: "traction on" picks the lowest-order mode that has TC (Sunburst: comfort, not the previous sport). The exe could pass a preferred mode in the rules.

## Phase 1 game-level commands (tested 2026-11-03)

### Repair / recover / respawn

The MCP tool names don't match the in-game meanings. These results were tested by dropping the car from 15 m and then calling each tool:

| Spoken intent | Call | Measured result |
|---|---|---|
| "repair", "fix", "restore", "recover", "get me back on the road" | MCP `recover_vehicle` | Repaired (damage → 0). Teleports to the last recorded road; with no roads nearby (small grid, open gridmap tarmac) it stays roughly in place. |
| "respawn", "reset to start", "go home" | `trigger_action {action:"loadHome"}` | Back at the level spawn point (the home point, unless the user ran `saveHome`), damage → 0. |
| (fallback for repair) | `trigger_action recover_vehicle_alt` | Repaired in place. |
| **Not for voice** | MCP `reload_vehicle` | Re-reads the jbeam from disk (a dev reload). Damage → ~0, stays in place, slow. |
| **Not for voice** | MCP `reset_vehicle` / `reset_physics` | Physics reset in place. Damage was **not** cleared in tests. |

Other game actions available via `trigger_action`: `saveHome`, `recover_to_last_road`, `reset_all_physics`, `dropPlayerAtCameraNoReset`.

### Levels

- `list_levels` returns names with translation-key titles. Get display names from `run_lua`:
  `for _,l in ipairs(core_levels.getList()) do translateLanguage(l.title,l.title,true) end`
  Examples: `west_coast_usa` = "West Coast, USA", `smallgrid` = "Grid, Small, Pure", `gridmap_v2` = "Gridmap v2", `beamng_earth` = "BeamNG.Earth", `italy`, `Utah` = "Utah, USA", `johnson_valley`, `jungle_rock_island`, `east_coast_usa`, `small_island` = "Small Island, USA", `Industrial` = "Industrial Site", `automation_test_track`, `hirochi_raceway`, `derby` = "Derby Arenas", `driver_training` = "ETK Driver Experience Center".
- `load_level {name}` took ~8 s on gridmap_v2. The MCP connection survives the load.
- Wait for the load by polling `get_status` until `.level` matches and `.vehicle` exists. The user's default vehicle spawns.
- Current level: `get_status.level`.
- Spawn points: `find_objects {class:"SpawnSphere"}` → named spheres. Gridmap has 12: `spawn_middle`, `spawn_offroad`, `spawn_drag`, …

### Vehicles

- Catalogue: `run_lua` → `core_vehicles.getModelList().models` gives 125 models, each with Brand, Name and Type.
- `core_vehicles.getModel(key)` gives `.model.default_pc` and `.configs[k].Configuration`, the display name. For example, LeGran (`legran`): default `s_v6_A`; 37 configs such as "Luxe V6 (A)", "Sport SE V6 AWD (Facelift) (M)", "Police Package (A)".
- `spawn {model}` with no config spawns the model's default (verified: legran → `s_v6_A`). `spawn {model, config}` replaces the current vehicle.
- **The game keeps no per-model history of configs used** (no such API in `core_vehicles`). The exe must remember "last config used" itself. Options: an auto-maintained `[history]` section in `config.toml`, or a file in the game's user folder written via MCP `write_file`.
- Config matching: fuzzy-match the spoken text against the `Configuration` display names; build the Whisper vocabulary from Brand + Name + config names.

### Sunburst centre diff (resolver action `center_diff`)

- `centerDiffModes` presets: auto / lock0 / lock50 / lock100. These drive `activeCenterDiffLock.minimumLock` = 0 / 0.5 / 1. Verified by reading the value back.
- `center_diff {percent=N}` picks the nearest preset (30% → 50%, 10% → 0%). `{mode="auto"}` returns to auto.
- Vehicles without such a controller return "unsupported" (verified on the D-Series).

### Traction control

"On" picks the lowest-order mode that has TC (Sunburst: Comfort). This is acceptable as the default.

## Rust exe — milestone 1: text mode (2026-11-03)

Decision: the last config used is kept **in memory only**, for the session.

Build: the user builds on Windows with `cargo build --release`. crates.io is blocked from Claude's sandboxes, so Claude couldn't compile the full crate.

- The intent parser was compiled and unit-tested standalone (27 phrases pass).
- Fuzzy matching was validated with a port of the same scoring against real model, config and level names.

Source layout:

| File | Role |
|---|---|
| `src/main.rs` | Startup: config, connect (retries until the game is up), tool check, catalogue, REPL or one-shot command. |
| `src/config.rs` | Commented default `config.toml` written beside the exe; user values deep-merged over the defaults. |
| `src/mcp.rs` | JSON-RPC over HTTP (`ureq`), `call` / `call_json` / `run_lua`, `vehicle_lua` (tag + poll; a non-noop, non-tag result counts as a Lua error), `show_message`. |
| `src/lua.rs` | `include_str!` of `lua/resolver.lua`, JSON → Lua literal, `resolve()` = act + deferred verify. |
| `src/catalog.rs` | Models (`core_vehicles`), configs per model, levels with translated titles; fuzzy matching (space-less Levenshtein for models; token overlap + Levenshtein for configs and maps; manual/auto → M/A). |
| `src/intents.rs` | Rule-based parser → `Intent` (stand-in for the router; same output type), plus `label` / `state_word` for messages. |
| `src/app.rs` | Executor: resolver intents; repair (`recover_vehicle`); respawn (`loadHome`); damage; spawn with last-config hint; list configs, cars and maps; current map; map loading with a progress message and wait. Message composition uses the config templates and icons. |

Menu over the game (tested 2026-11-03):

- `get_ui_state` → `route.currentRoute.resolved.name` is `"play"` in game, and `"menu"` or `"pause"` when those screens are up.
- `set_ui_state {route:"play"}` closes them. Sometimes this goes menu → pause → play, so it needs a retry loop.
- `ensure_play()` loops up to 5× until the route is "play", then calls `resume_physics` if physics is paused.
- It runs only when a level and vehicle are loaded, so it never leaves the real main menu.
- Called at startup, after every map load (1.5 s after the map reports loaded), and by the "close menu" / "back to game" / "unpause" command.
- Not reproduced: the menu the user sees staying up after a load. Navigating to `menu.mainmenu` while in a map shows nothing and reports "play". If the stuck menu reports "play" too, it needs a DOM check via `ui_eval`.

Pause after map load and spawn (found 2026-11-03):

- After loading a map and then spawning, the game was on the "pause" route with `simTimeAuthority.getPause()` = true and `timeScale` 0, while `physicsRunning` stayed **true**.
- The player vehicle itself was normal: id 33139, object name "thePlayer", which is how levels name their starting car.
- Fix: `ensure_play` first checks `simTimeAuthority.getPause()` and calls `simTimeAuthority.pause(false)`, then navigates the route to "play". Verified live.
- `ensure_play` now also runs 1 s after every spawn.

"Vehicle 33139" name in the pause menu (found 2026-11-03):

- Source: the pause menu vehicle card (`PauseVehicleSpawnedTileGrid.vue`) shows `rawName` from `core_quickAccess.getVehicleName(veh)`.
- When `core_quickAccess` isn't loaded, `vehicleTabInteractions.lua` falls back to `ui.pause.vehicle.fallbackName` = "Vehicle {id}".
- After a map load via MCP `load_level`, `core_quickAccess` (radial menu) was **not loaded**. Remove-then-spawn still showed "Vehicle 35040".
- Fix: `extensions.load('core_quickAccess')`. The card then showed "Hirochi Sunburst 2.5 Sport RS AWD (DCT)".
- The exe does this in `ensure_play` (startup, after map load, after spawn).
- Object names "thePlayer" (level start car) and "clone" (spawned) are normal and not shown to the user.

Paint (`src/paint.rs`, tested live 2026-11-03):

- Per-vehicle paints: `core_vehicle_manager.getVehicleData(id).config.paints[1..3].baseColor`. Slot 1 is the body; 2 and 3 are accents.
- Factory paints: `core_vehicles.getModel(jbeam).model.paints[name]` (33 on the Sunburst).
- Apply: `core_vehicle_colors.setVehiclePaint(slot, paintTable, vehId)`, which also saves the paint to the vehicle's config.
- Colour classification is HSV with absolute-chroma neutrals. It names all 33 Sunburst factory paints sensibly.
- Vehicle selection:
  - Filter by model (fuzzy) and current body colour.
  - Neither given → the player vehicle.
  - Several matches → the player vehicle first, then the nearest one; the message reports how many were left as is.
- Paint selection, in order:
  1. Exact factory paint name.
  2. Names containing every spoken word ("black" → Black Diamond).
  3. Any factory paint of that colour.
  4. A custom paint.
  - Matte and chrome finishes are avoided unless asked for.
- Slots 2 and 3 are repainted only if they matched the body colour, so accents are kept.
- Verified by reading the paint back.

Environment (`src/env.rs`, tested live 2026-11-03):

- `core_environment.getState()` gives time, play, dayLength, cloudCover, fogDensity (×1000 in state; the getter returns the raw value), windSpeed, temperatureC, gravity.
- **Native time of day: 0 = noon, 0.5 = midnight.** The MCP `set_time_of_day` description says the opposite, so it isn't used. Set time with `core_environment.setTimeOfDay({time=..})`; partial tables work for play and dayLength too.
- Named times are per level: `getTimeOfDayOptions()` (sunrise, morning, earlyNoon, noon, lateNoon, afternoon, evening, sunset, night).
- Setters: `setCloudCover(0..1)`, `setFogDensity` (raw; the exe treats 0.01 as 100%), `setWindSpeed` (m/s), `setGravity` (presets from `getGravityPresets`), `setPrecipitation(numDrops)`.
  - Rain needs `Precipitation` objects. Levels that have them (found by searching level files): beamng_earth, driver_training, east_coast_usa, Industrial, italy, jungle_rock_island, small_island, west_coast_usa. **Neither gridmap nor smallgrid has rain.**
  - Small Island has two objects: `rain_single_drops` (datablock `rain_drop`) and `rain_coverage` (`rain_medium`, sheet rain). `core_environment.setPrecipitation` only sets the first.
  - The exe sets the drop layer to N and other layers to N/2 (light 400, medium 1200, heavy 3000), and raises clouds to ≥85% unless the sky was mentioned.
  - Verified on Small Island: object values read back, and a screenshot shows denser rain with both layers set.
- `core_weather.switchWeather(preset)` **fails** in this build: `weather.lua:92` indexes missing object fields. Weather is set directly instead.
- Temperature follows the level's temperature profile; `setState{temperatureC}` had no effect. It is reported read-only.
- Sim speed: MCP `set_simulation_speed {scale}`.
- The "what can I change" list shows every variable with its current value, plus the level's named times.

Traffic and AI (`src/ai.rs`, tested live 2026-11-03):

- Traffic: `gameplay_traffic.setupTraffic(n, {keepCurrent=true})`; n = -1 means auto. Ordinary traffic uses `simple_traffic` configs (e.g. `fullsize_taxi`, `van_passenger_facelift`).
- Police: `gameplay_traffic_trafficUtils.createPoliceGroup(n)` passed as `vehGroup`. Asking for 1 gave 2 Wendover Police Interceptors, so the reply reports actual counts.
- Remove: `gameplay_traffic.deleteVehicles()`. This keeps the player car.
- Per-car behaviour: MCP `set_ai {id, mode, targetObjectID, aggression, routeSpeed, speedMode}`. Modes: chase, follow, flee, stop, traffic, random, disabled. Verified chase on a taxi.
- Police chasing the player use `gameplay_police.setPursuitMode(1, playerId, {policeIds})`.
- Description matching:
  - Each vehicle's description is built from its name (`core_quickAccess.getVehicleName`, minus "Simplified Traffic Vehicles"), its body style (simple_traffic configs take the real model's body style from the config prefix), Type, traffic role and config key.
  - Body colour is matched with the paint classifier.
  - Synonyms: cop → police, ute → pickup, cab → taxi.
  - The nearest match is used, or every match with "all". The player car is excluded except for "autopilot" / "my car".
- AI spawn: `spawn {add:true}` takes over the player; the exe re-enters the previous car with `be:enterVehicle(0, obj)` and sets the new car to traffic mode. Verified.

Help (`src/help.rs`):

- Triggers: "help", "command(s)", "what can you do", "what can I ask/say".
- A category word picks one section: vehicle, spawn/cars, map/level, weather/time, traffic/AI/police, game. With none, the reply gives an overview of the categories.
- "what weather settings can I change" still goes to the environment list, which shows live values.
- The full guide prints to the console; the in-game message shows one section's examples.

Models (`src/models.rs`, 2026-11-03):

- `[models] dir` (default "models", relative to the exe or absolute) plus per-model `file`, optional `path` override, `url` and `sha256`.
- `ModelsCfg` uses `#[serde(flatten)]` to collect the `[models.<name>]` tables.
- The startup log lists full paths and download status. The `models` prompt command does the same.
- `--models` downloads missing files (resumable via `.part` + HTTP Range, with progress) and verifies sha256 if set.
- URLs checked to exist on Hugging Face: whisper.cpp `ggml-base.en.bin`; ggml-org/whisper-vad `ggml-silero-v5.1.2.bin` (v6.2.0 also available); CompendiumLabs `bge-small-en-v1.5-q8_0.gguf`; Qwen `Qwen3-1.7B-Q8_0.gguf` (1.83 GB).
- LLM default changed to bartowski `Qwen_Qwen3-0.6B-Q4_K_M.gguf` (0.48 GB). The LLM only classifies and fills slots with grammar-constrained output, so 0.6B Q4 is enough; 1.7B Q8 is noted as the bigger alternative in the config.
- New dependency: `sha2`.

Voice, stage A (`src/voice.rs`, written 2026-11-03; first build pending):

- **Library choice:**
  - `whisper-rs` 0.16 handles speech-to-text and whisper.cpp's Silero VAD.
  - The embedding model and LLM will run on **Candle** 0.11 (pure Rust: `bert` + `quantized_qwen3`), not llama.cpp. `whisper-rs` and `llama-cpp-2` each bundle their own ggml, and the symbols clash in one binary (llama-cpp-rs issue #884).
  - `tokenizers` 0.23 builds without C (`default-features = false, features = ["fancy-regex"]`).
- **APIs, checked on docs.rs:**
  - whisper-rs: `WhisperContext::new_with_params(path, WhisperContextParameters)`, `create_state`, `FullParams::new(SamplingStrategy::Greedy{best_of})` and its setters, `state.full(params, &pcm)`, `state.as_iter()` → `seg.to_str_lossy()`.
  - whisper-rs VAD: `WhisperVadContext::new(&str, WhisperVadContextParams)`, `segments_from_samples(WhisperVadParams, &[f32])`.
  - cpal 0.18: `SampleRate = u32`; `build_input_stream(StreamConfig, data_cb, err_cb, Option<Duration>)`; the device name API is `description()`.
- **Pipeline:**
  1. cpal reads the default microphone, mixes to mono and resamples to 16 kHz (box-filter decimation).
  2. Endpointing works on 30 ms frames against an adaptive noise floor: speech starts after 3 loud frames, with 400 ms of pre-roll, and ends after `end_silence_ms`.
  3. Silero VAD must find speech, otherwise the clip is dropped.
  4. Whisper runs (English, greedy, single segment, no timestamps, initial prompt = vocabulary).
  5. Known hallucinations ("Thank you.", "[BLANK_AUDIO]") are dropped.
  6. The gate decides whether it's a command (hotword / both / open, and the listen window after a bare wake phrase).
  7. The command goes to the same `App::handle` as typed text.
- **Threads:** the main loop receives `voice::Input::{Text, Voice, Quit}` from a stdin thread and the voice thread.
- **Startup:** `models::ensure_some(["stt","vad"])` downloads only what voice needs and prints full paths.
- **Config:** `[voice]` replaces `[wake]`. The embed/llm model entries changed to Candle formats (safetensors + config + tokenizer; GGUF + tokenizer).

GPU (`src/gpu.rs`, 2026-11-03):

- Speech uses whisper.cpp's **Vulkan** backend (`whisper-rs/vulkan`, cargo feature `gpu`). One build covers NVIDIA, AMD and Intel through the driver's `vulkan-1.dll`. The build needs the Vulkan SDK; `build.cmd` enables the feature when `VULKAN_SDK` is set.
- **Rejected:**
  - CUDA / hipblas: they need several hundred MB of vendor DLLs beside the exe.
  - OpenVINO: whisper-rs 0.16 has no OpenVINO feature (features: coreml, cuda, hipblas, intel-sycl, metal, openblas, openmp, vulkan).
- Free VRAM comes from DXGI `IDXGIAdapter3::QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL)` → Budget − CurrentUsage (windows 0.62; API checked on microsoft.github.io/windows-docs-rs). Software adapters are skipped.
- `[voice] device` options:
  - `auto`: GPU if free ≥ need (2 × model size + 300 MB) + `vram_headroom_mb`.
  - `gpu`: forced.
  - `cpu`.
- `gpu_device` sets the ggml GPU index. If the GPU context fails to start, it reloads on CPU.
- Router and LLM (Candle, stages B/C): Candle's GPU backends are CUDA and Metal only, with no AMD/Vulkan/OpenVINO. Plan: CPU by default (the embedding takes ~10 ms; the 0.6B LLM only runs on tie-breaks). Possible later: an optional `cuda` feature with `[router] device = auto|cuda|cpu`, which would need CUDA DLLs on NVIDIA systems.

Dependencies: `anyhow`, `serde`, `serde_json`, `toml`, `ureq` (rustls), `strsim`. Static CRT: `build.cmd` sets `RUSTFLAGS=+crt-static` (remote tools can't write `.cargo/`).

## Still to do

- [ ] Probe an EV (`electricMotor` devices, no gearbox modes, regen and drive modes).
- [ ] Probe a manual Percheron and an AWD with a lockable centre diff via a device rather than a controller.
- [x] Lua resolver library v0.1 (`lua/resolver.lua`).
- [ ] Resolver: EV support (motor modes, regen), couplers (doors/tailgate), parking brake, ignition/engine start, `airSuspension`/`compressionBrake`/`retarder` on trucks, damage summary.
- [ ] "List all switchable electrics" — derive from `electrics` setters plus `driveModes` and coupler controllers.
- [ ] Damage overview: `get_vehicle_damage` plus `beamstate`/`damageTracker` detail in the vehicle VM.
- [x] Time, weather, gravity, sim speed; traffic and police spawn; per-car AI behaviour by description.
- [ ] Map objects: `find_objects {class:'SpawnSphere'}` and notable locations (e.g. fuel stations) → `drive_to` or satnav (to investigate).
- [ ] Map and vehicle management: `get_status`, `list_levels`, `load_level`, `spawn`, model/config vocabulary for Whisper and the router.
- [ ] Rust scaffold: MCP client + async vlua helper + `show_message` composer (text-command mode first).

## Files

- `lua/probe_powertrain.lua` — dumps devices, controllers and electrics (used for the findings above).
- `lua/probe_drivemodes.lua` — dumps every `driveModes` controller and what each mode sets.
- `lua/resolver.lua` — the resolver library (see above).

## 0.2.0 (dev branch, 2026-11-04)

Built while the game PC was offline: everything compiles (`cargo check`), the parser and place-matching tests pass, and the new resolver actions were run in LuaJIT against a mock vehicle. Then built and tested in the game on Windows, and released as v0.2.0.

- **Map loading** now uses `freeroam_freeroam.startFreeroamByName(name)` instead of MCP `load_level`. It loads the full freeroam extension set (core_multiSpawn, gameplay_rawPois, core_quickAccess…) and ends on "play", unpaused. The exe polls `get_status` every 1.5 s, treats timeouts as "still loading" (the game stops answering for ~10–20 s on West Coast USA), and finishes after two answers in a row showing the new level and a vehicle.
- **Missing extensions:** `places::ENSURE_EXT` loads core_quickAccess, core_multiSpawn, gameplay_traffic, gameplay_police, gameplay_rawPois and core_groundMarkers if absent. It runs in `ensure_play`, before traffic/police spawns, and before every place command. This fixes "attempt to index global 'core_multiSpawn' (a nil value)" when spawning police.
- **"spawn ai" / "spawn 3 ai cars"** with no model now spawn traffic.
- **Pause:** "pause the game" → `simTimeAuthority.pause(true)` (the J key). "unpause" still goes through `ensure_play`. A vehicle command that times out while the game is paused now says so.
- **Places (`src/places.rs`):** `gameplay_rawPois.getRawPoiListByLevel(level)` minus missions: spawn points, gas/charging stations, drag strips. Names via `translateLanguage`, positions from `markerInfo.bigmapMarker.pos`. "(Rules & Setup)" duplicates are dropped. Matching: exact name, then category + nearest ("nearest gas station"), then fuzzy (token overlap + Levenshtein; space-less near-exact counts on its own, for "china town").
- **Satnav:** `core_groundMarkers.setPath(vec3, {clearPathOnReachingTarget=true})`, road length from `getPathLength()` ~400 ms later, `setPath(nil)` to clear. Distance and compass are straight-line from the player (+x east, +y north).
- **Autopilot:** "autopilot on" reads `core_groundMarkers.getTargetPos()` and calls MCP `drive_to {pos, driveInLane:"on"}`. With no route it falls back to `set_ai mode=traffic`. "autopilot off" → `set_ai mode=disabled`. "take me to X" sets the route, then drives it.
- **Teleport:** MCP `set_position {pos}` (+0.5 m z; keeps heading, repairs).
- **Resolver actions:** `body` (advancedCouplerControl `toggleGroup`/`getGroupState`; the hood latch + catch pair counts as one panel; default door is FL), `engine` (ignition 2 + `controller.mainController.setStarter(true)`; verify passes release the starter once the engine runs, or after ~6 s), `jato` (`electrics.values.jatoInput`), `nitrous` (`electrics.values.nitrousOxideArm`), `engine_brake` (compressionBrake level/coef 0.33/0.66/1), `retarder` (setting index 1 auto, 2 off, 3–7 = 20–100%), `parking_brake` (`input.event('parkingbrake', v, FILTER_DIRECT)`), `air_suspension` (setMomentaryIncrease/Decrease; **semantics unverified**).
- **Verify loop:** results may set `verify_ms` (doors 1800, engine 1500, parking brake 800). A verify that returns "pending" again is re-run every 1.5 s, up to 5 passes.
- **Messages:** new `ttl_info` (default 15 s) for satnav, distances and autopilot replies.

## Planned after stage C (not for the README)

- **Unprompted reactions.** Deliberately not described in the README; the only mention there is the "never sends a message unprompted… probably" line in the Terms of cover.
  - **What:** an occasional one-line message reacting to what the player is doing, in the active personality's voice. Examples: "This game is more than crashes, you know." / "The D-Series wasn't designed for that." / "Would you like me to show you how to find the brake pedal?"
  - **Rate:** at most once every 2 hours, with no minimum. Hours with nothing worth saying are fine.
  - **Triggers:** only real in-game events that would be very unusual in the real world. Examples: a large jump in damage, extreme speed for the vehicle type, big airtime or altitude change, rolling over, driving into water, repeated respawns. Each event gets a significance score; only a high score, outside the cooldown, can fire.
  - **How:** a low-rate telemetry sample (about every 2 s) of damage, speed, altitude and orientation. It runs only while a vehicle is loaded, and can later share the HTTP API's live-state reader.
  - **Wording, written by the stage C LLM:** the code detects the event; the LLM only writes the line. It gets a short fact sheet (event type, vehicle name and type, the numbers involved such as speed, height or damage, and the personality), plus a few example lines, and writes one sentence of at most ~15 words.
    - It runs in the background with no time pressure, and only when something has already passed the significance and cooldown checks, so a slow CPU-only model is fine.
    - The output is checked (length, one line, no instructions or offensive words). If it fails, or stage C is turned off, a line from a built-in pool per event type is used instead.
    - This is the one place the LLM writes text. Command replies stay template-based.
  - **Config:** `[reactions] enabled = true`, `min_gap_minutes = 120`.
