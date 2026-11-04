-- BeamNG.Speak resolver library (vehicle Lua VM). Stateless.
-- The exe prepends this file to every command and appends one line:
--   return BNGS.call("<action>", {<args>}, {<rule overrides>}, "<tag>")
-- The result is "<tag>" .. jsonEncode(result), where result is
--   { status = applied|already|partial|unsupported|failed|pending,
--     action, targets = {{role, device, ui, from, to, want, ok}},
--     side = {{device, from, to}},      -- non-target devices that changed
--     checks = {...},                   -- deferred checks; the exe sends "verify" ~300 ms later
--     detail, data }

local BNGS = {}

---------------------------------------------------------------- rules
BNGS.defaults = {
  lockedModes   = { locked = true, lock = true },
  unlockedPref  = { "open", "lsd", "viscous", "unlocked" },
  -- shiftToGearIndex mapping per gearbox type (verified: dctGearbox, realistic behaviour)
  gearMap = {
    automaticGearbox = { R = -1, N = 0, P = 1, D = 2, S = 3 },
    dctGearbox       = { R = -1, N = 0, P = 1, D = 2, S = 3 },
    cvtGearbox       = { R = -1, N = 0, P = 1, D = 2, S = 3 },
    sequentialGearbox= { R = -1, N = 0 },
    manualGearbox    = { R = -1, N = 0 },
  },
  scopes = {
    front  = { "diff_front", "inter_front" },
    rear   = { "diff_rear", "inter_rear" },
    center = { "center" },
    inter  = { "inter_front", "inter_rear", "center" },
    axles  = { "diff_front", "diff_rear" },
    all    = { "diff_front", "diff_rear", "inter_front", "inter_rear", "center" },
  },
  -- driveModes controllers that act as a centre diff (e.g. Sunburst centerDiffModes)
  centerCtrlPattern = "center",
  centerCtrlModes = { locked = { "lock100", "locked", "lock" }, open = { "auto", "lock0", "unlocked", "open" } },
  tcOffPattern = "off",
  readGroups = {
    temps  = { match = "temp" },
    fuel   = { keys = { "fuel", "fuelVolume", "fuelCapacity", "lowfuel" } },
    engine = { keys = { "rpm", "engineRunning", "engineLoad", "ignitionLevel", "oil", "checkengine" } },
    speed  = { keys = { "wheelspeed", "airspeed" } },
    lights = { keys = { "lights_state", "highbeam", "fog", "hazard_enabled", "signal_left_input", "signal_right_input" } },
    assist = { keys = { "hasABS", "abs", "hasESC", "esc", "hasTCS", "tcs" } },
  },
}

local R = BNGS.defaults
local function mergeRules(over)
  if type(over) ~= "table" then return end
  for k, val in pairs(over) do R[k] = val end
end

---------------------------------------------------------------- helpers
local function has(list, x) for _, y in ipairs(list or {}) do if y == x then return true end end return false end
local function lower(s) return type(s) == "string" and s:lower() or "" end

local function setDevMode(name, mode)
  if powertrain.setDeviceMode then powertrain.setDeviceMode(name, mode)
  else powertrain.getDevices()[name]:setMode(mode) end
end

local function snapshotModes()
  local s = {}
  for n, d in pairs(powertrain.getDevices()) do if d.mode ~= nil then s[n] = d.mode end end
  return s
end

-- wheels and diff count below a device
local function below(d)
  local acc = { wheels = {}, diffs = 0, ends = {} }
  local function walk(x)
    if x.connectedWheel then
      table.insert(acc.wheels, x.connectedWheel)
      local e = x.connectedWheel:sub(1, 1)
      if e == "F" or e == "R" then acc.ends[e] = true end
    end
    for _, c in ipairs(x.children or {}) do
      if c.type == "differential" then acc.diffs = acc.diffs + 1 end
      walk(c)
    end
  end
  walk(d)
  return acc
end

-- Classify a switchable device. Returns nil if the device is not switchable.
function BNGS.classify(d)
  local modes = d.availableModes or {}
  if #modes < 2 then return nil end
  local w = below(d)
  local loc = (w.ends.F and w.ends.R) and "center" or (w.ends.F and "front") or (w.ends.R and "rear") or "none"
  if d.type == "rangeBox" then return { role = "range", loc = loc } end
  local canLock = false
  for _, m in ipairs(modes) do if R.lockedModes[m] then canLock = true end end
  if d.type == "differential" and canLock then
    if loc == "center" then return { role = "diff", cls = "center", loc = loc } end
    if w.diffs >= 2 then return { role = "diff", cls = "inter_" .. loc, loc = loc } end
    return { role = "diff", cls = "diff_" .. loc, loc = loc }
  end
  if has(modes, "connected") and has(modes, "disconnected") then
    if #w.wheels == 1 then return { role = "hub", loc = loc } end
    return { role = "drive", loc = loc }
  end
  return { role = "other", loc = loc }
end

function BNGS.switchables()
  local out = {}
  for n, d in pairs(powertrain.getDevices()) do
    local c = BNGS.classify(d)
    if c then c.device = n; c.ui = d.uiName; c.mode = d.mode; c.modes = d.availableModes; c.type = d.type; out[n] = c end
  end
  return out
end

-- driveModes controllers and what each mode sets
function BNGS.driveModeCtrls()
  local list = {}
  for n, c in pairs(controller.getAllControllers()) do
    if c.typeName == "driveModes" and v.data[n] and v.data[n].modes then
      local modes = {}
      for k in pairs(v.data[n].modes) do
        local ok, dd = pcall(c.getDriveModeData, k)
        if ok and dd then
          local dev, ctl = {}, {}
          for sk, st in pairs(dd.settings or {}) do
            if st.type == "powertrainDeviceMode" then dev[st.deviceName] = st.mode
            elseif st.type == "controller" then ctl[st.controllerName or st.name or sk] = true end
          end
          modes[k] = { key = k, name = dd.name, order = dd.order or 0, dev = dev, ctl = ctl }
        end
      end
      list[n] = { c = c, name = n, cur = c.getCurrentDriveModeKey and c.getCurrentDriveModeKey(), modes = modes }
    end
  end
  return list
end

-- Desired mode for a device given an abstract state
local function pickMode(d, state)
  local modes = d.availableModes or {}
  if state == "locked" then
    for _, m in ipairs(modes) do if R.lockedModes[m] then return m end end
    return nil
  end
  if state == "open" then
    if d.defaultMode and not R.lockedModes[d.defaultMode] and has(modes, d.defaultMode) then return d.defaultMode end
    for _, p in ipairs(R.unlockedPref) do if has(modes, p) then return p end end
    for _, m in ipairs(modes) do if not R.lockedModes[m] then return m end end
    return nil
  end
  if has(modes, state) then return state end
  return nil
end

-- Choose driveModes controller modes covering the device targets; leftovers go direct.
local function plan(want, need, ctrls)
  local remaining = {}
  for k, val in pairs(need) do remaining[k] = val end
  local cur = snapshotModes()
  local steps = {}
  while next(remaining) do
    local best
    for _, cc in pairs(ctrls) do
      if not cc.used then
        for k, m in pairs(cc.modes) do
          local hits, bad, side = 0, false, 0
          for dn, dm in pairs(m.dev) do
            if want[dn] then
              if want[dn] == dm then if remaining[dn] then hits = hits + 1 end else bad = true end
            elseif cur[dn] ~= nil and cur[dn] ~= dm then side = side + 1 end
          end
          if hits > 0 and not bad then
            local score = hits * 10 - side
            if not best or score > best.score then best = { cc = cc, key = k, m = m, score = score } end
          end
        end
      end
    end
    if not best then break end
    best.cc.used = true
    for dn in pairs(best.m.dev) do remaining[dn] = nil end
    table.insert(steps, { kind = "driveMode", ctrl = best.cc.name, key = best.key, c = best.cc.c })
  end
  for dn, dm in pairs(remaining) do table.insert(steps, { kind = "device", dev = dn, mode = dm }) end
  return steps
end

local function finish(res)
  local n, ok = #res.targets, 0
  for _, t in ipairs(res.targets) do if t.ok then ok = ok + 1 end end
  if res.status then return res end
  if n == 0 then res.status = "unsupported"
  elseif ok == n then res.status = "applied"
  elseif ok > 0 then res.status = "partial"
  else res.status = "failed" end
  return res
end

-- Generic device-target executor: want = {deviceName = mode}, roles = {deviceName = roleLabel}
local function applyDeviceTargets(action, want, roles)
  local devs = powertrain.getDevices()
  local before = snapshotModes()
  local res = { action = action, targets = {}, side = {}, steps = {} }
  local need = {}
  for dn, dm in pairs(want) do if before[dn] ~= dm then need[dn] = dm end end
  if not next(want) then res.status = "unsupported"; return res end
  if not next(need) then
    res.status = "already"
    for dn, dm in pairs(want) do
      table.insert(res.targets, { role = roles[dn], device = dn, ui = devs[dn].uiName, from = dm, to = dm, want = dm, ok = true })
    end
    return res
  end
  local steps = plan(want, need, BNGS.driveModeCtrls())
  for _, s in ipairs(steps) do
    local ok, err
    if s.kind == "driveMode" then ok, err = pcall(s.c.setDriveMode, s.key)
    else ok, err = pcall(setDevMode, s.dev, s.mode) end
    table.insert(res.steps, { kind = s.kind, ctrl = s.ctrl, key = s.key, dev = s.dev, mode = s.mode, ok = ok, err = (not ok) and tostring(err) or nil })
  end
  local after = snapshotModes()
  for dn, dm in pairs(want) do
    table.insert(res.targets, { role = roles[dn], device = dn, ui = devs[dn].uiName, from = before[dn], to = after[dn], want = dm, ok = after[dn] == dm })
  end
  for dn, m in pairs(after) do
    if not want[dn] and before[dn] ~= m then table.insert(res.side, { device = dn, ui = devs[dn].uiName, from = before[dn], to = m }) end
  end
  return finish(res)
end

-- Controller-only fallback for a role (e.g. Sunburst centerDiffModes)
local function ctrlFallback(action, pattern, keysByState, state)
  for _, cc in pairs(BNGS.driveModeCtrls()) do
    if lower(cc.name):find(pattern) then
      for _, k in ipairs(keysByState[state] or {}) do
        if cc.modes[k] then
          local res = { action = action, targets = {}, side = {} }
          if cc.cur == k then res.status = "already" else cc.c.setDriveMode(k) end
          local now = cc.c.getCurrentDriveModeKey()
          table.insert(res.targets, { role = "controller", device = cc.name, ui = cc.modes[k].name, from = cc.cur, to = now, want = k, ok = now == k })
          return finish(res)
        end
      end
    end
  end
  return nil
end

---------------------------------------------------------------- actions
local A = {}

-- args: scope = front|rear|center|inter|axles|all, state = locked|open
function A.diff_lock(args)
  local scope = R.scopes[args.scope or "all"] or R.scopes.all
  local want, roles = {}, {}
  for n, c in pairs(BNGS.switchables()) do
    if c.role == "diff" and has(scope, c.cls) then
      local m = pickMode(powertrain.getDevices()[n], args.state)
      if m then want[n] = m; roles[n] = c.cls end
    end
  end
  if not next(want) and has(scope, "center") then
    local r = ctrlFallback("diff_lock", R.centerCtrlPattern, R.centerCtrlModes, args.state == "locked" and "locked" or "open")
    if r then return r end
  end
  return applyDeviceTargets("diff_lock", want, roles)
end

-- Active centre diff with preset lock levels (e.g. Sunburst centerDiffModes: auto, lock0, lock50, lock100).
-- args: percent = 0..100, or mode = "auto". Picks the nearest available preset.
function A.center_diff(args)
  for _, cc in pairs(BNGS.driveModeCtrls()) do
    if lower(cc.name):find(R.centerCtrlPattern) then
      local key, label
      if args.mode == "auto" or args.percent == nil then
        key = cc.modes.auto and "auto" or nil
      else
        local bestD
        for k in pairs(cc.modes) do
          local n = tonumber(k:match("(%d+)$"))
          if n and (not bestD or math.abs(n - args.percent) < bestD) then bestD = math.abs(n - args.percent); key = k end
        end
      end
      local res = { action = "center_diff", targets = {}, side = {} }
      if not key then res.status = "unsupported"; res.detail = "no matching centre diff mode"; return res end
      label = cc.modes[key].name
      if cc.cur == key then res.status = "already" else cc.c.setDriveMode(key) end
      local now = cc.c.getCurrentDriveModeKey()
      local lock
      local a = controller.getController("activeCenterDiffLock")
      if a and a.getConfig then lock = a.getConfig().minimumLock end
      table.insert(res.targets, { role = "center", device = cc.name, ui = label, from = cc.cur, to = now, want = key, ok = now == key, minimumLock = lock })
      return finish(res)
    end
  end
  return { action = "center_diff", status = "unsupported", targets = {}, side = {} }
end

-- args: state = low|high
function A.range(args)
  local want, roles = {}, {}
  for n, c in pairs(BNGS.switchables()) do
    if c.role == "range" then
      local m = pickMode(powertrain.getDevices()[n], args.state)
      if m then want[n] = m; roles[n] = "range" end
    end
  end
  return applyDeviceTargets("range", want, roles)
end

-- args: state = 4wd|2wd
function A.drive(args)
  local mode = (args.state == "2wd") and "disconnected" or "connected"
  local want, roles = {}, {}
  for n, c in pairs(BNGS.switchables()) do
    if c.role == "drive" or c.role == "hub" then want[n] = mode; roles[n] = c.role end
  end
  return applyDeviceTargets("drive", want, roles)
end

-- Electrics: changes are verified in a deferred "verify" pass
local function eTarget(action, key, cur, want, fn)
  local res = { action = action, targets = { { role = "electrics", device = key, from = cur, want = want } }, side = {} }
  if cur == want then res.targets[1].to = cur; res.targets[1].ok = true; res.status = "already"; return res end
  local ok, err = pcall(fn)
  if not ok then res.status = "failed"; res.detail = tostring(err); return res end
  res.status = "pending"
  res.checks = { { src = "e", key = key, expect = want } }
  return res
end

-- args: state = off|low|high
function A.lights(args)
  local map = { off = 0, low = 1, on = 1, high = 2 }
  local n = map[args.state]
  if not n then return { action = "lights", status = "failed", detail = "bad state" } end
  return eTarget("lights", "lights_state", electrics.values.lights_state, n, function() electrics.setLightsState(n) end)
end

local function onoff(s) return (s == "on" or s == true or s == 1) and 1 or 0 end

function A.fog(args)
  if electrics.values.fog == nil then return { action = "fog", status = "unsupported", targets = {} } end
  local n = onoff(args.state)
  return eTarget("fog", "fog", electrics.values.fog, n, function() electrics.set_fog_lights(n) end)
end

function A.hazard(args)
  local n = onoff(args.state)
  return eTarget("hazard", "hazard_enabled", electrics.values.hazard_enabled, n, function() electrics.set_warn_signal(n) end)
end

-- args: side = left|right|off. set_*_signal is unreliable, so read the state and toggle.
function A.signal(args)
  local e = electrics.values
  local wantL = args.side == "left" and 1 or 0
  local wantR = args.side == "right" and 1 or 0
  local res = { action = "signal", targets = {
    { role = "electrics", device = "signal_left_input", from = e.signal_left_input, want = wantL },
    { role = "electrics", device = "signal_right_input", from = e.signal_right_input, want = wantR } }, side = {} }
  if e.signal_left_input == wantL and e.signal_right_input == wantR then
    for _, t in ipairs(res.targets) do t.to = t.from; t.ok = true end
    res.status = "already"; return res
  end
  if e.signal_left_input ~= wantL then electrics.toggle_left_signal() end
  if e.signal_right_input ~= wantR and not (wantR == 0 and wantL == 1) then electrics.toggle_right_signal() end
  res.status = "pending"
  res.checks = { { src = "e", key = "signal_left_input", expect = wantL }, { src = "e", key = "signal_right_input", expect = wantR } }
  return res
end

-- args: state = on|off
function A.traction(args)
  local tc = controller.getController("tractionControl")
  local want = args.state ~= "off"
  if not tc then
    return { action = "traction", status = "unsupported", targets = {}, detail = (electrics.values.hasTCS == 1) and "no tractionControl controller" or nil }
  end
  local cur = tc.getConfig().isEnabled
  local res = { action = "traction", targets = { { role = "controller", device = "tractionControl", from = cur, want = want } }, side = {} }
  if cur == want then res.targets[1].to = cur; res.targets[1].ok = true; res.status = "already"; return res end
  local pick
  for _, cc in pairs(BNGS.driveModeCtrls()) do
    for k, m in pairs(cc.modes) do
      if m.ctl.tractionControl then
        local isOff = lower(k):find(R.tcOffPattern) or lower(m.name):find(R.tcOffPattern)
        if (not want and isOff) or (want and not isOff) then
          if not pick or m.order < pick.m.order then pick = { cc = cc, k = k, m = m } end
        end
      end
    end
  end
  if not pick then res.status = "unsupported"; res.detail = "no drive mode toggles traction control"; return res end
  pick.cc.c.setDriveMode(pick.k)
  res.via = pick.cc.name .. ":" .. pick.k
  res.status = "pending"
  res.checks = { { src = "ctrl", ctrl = "tractionControl", expect = want } }
  return res
end

-- electrics.values.gear is a letter on automatics ("P", "D", "S1") but a number on manuals (0 = N, -1 = R)
local function gearName(g)
  if type(g) == "number" then
    if g == 0 then return "N" elseif g < 0 then return "R" else return tostring(g) end
  end
  return tostring(g)
end

-- args: state = P|R|N|D|S or a gear number (manual)
function A.gear(args)
  local gb = powertrain.getDevices().gearbox
  if not gb then return { action = "gear", status = "unsupported", targets = {} } end
  local s = tostring(args.state):upper()
  local map = R.gearMap[gb.type] or {}
  local idx = map[s] or tonumber(s)
  local cur = gearName(electrics.values.gear)
  local res = { action = "gear", targets = { { role = "gear", device = gb.type, from = cur, want = s } }, side = {} }
  if not idx then res.status = "unsupported"; res.detail = "no gear " .. s .. " on " .. gb.type; return res end
  if tostring(cur):sub(1, #s) == s then res.targets[1].to = cur; res.targets[1].ok = true; res.status = "already"; return res end
  controller.mainController.shiftToGearIndex(idx)
  res.status = "pending"
  res.checks = { { src = "gear", expect = s } }
  return res
end

---------------------------------------------------------------- body: doors, hood, boot
-- Openable panels are advancedCouplerControl controllers (door_FL_coupler, hoodLatchCoupler +
-- hoodCatchCoupler, trunkCoupler...). toggleGroup() opens/closes (closing re-latches over ~1.5 s);
-- getGroupState() is "attached" when shut.
local function panelKind(name)
  local n = lower(name)
  if n:find("door") then
    local pos = n:match("door_?(%a%a)_") or ""
    return "door", pos:upper()
  end
  if n:find("hood") or n:find("bonnet") then return "hood", "" end
  if n:find("trunk") or n:find("tailgate") or n:find("boot") or n:find("hatch") or n:find("liftgate") then return "trunk", "" end
  return "other", ""
end

function BNGS.panels()
  local out = {}
  for n, c in pairs(controller.getAllControllers()) do
    if c.typeName == "advancedCouplerControl" and c.toggleGroup and c.getGroupState then
      local kind, pos = panelKind(n)
      table.insert(out, { name = n, kind = kind, pos = pos, c = c })
    end
  end
  table.sort(out, function(a, b) return a.name < b.name end)
  return out
end

-- which: "" | all | FL FR RL RR | front rear | left right
local function panelMatch(p, part, which)
  if part == "all" then return true end
  if part ~= "door" and part ~= "doors" then return p.kind == part end
  if p.kind ~= "door" then return false end
  if which == "" or which == "all" then return true end
  if #which == 2 then return p.pos == which:upper() end
  if which == "front" or which == "rear" then return p.pos:sub(1, 1) == (which == "front" and "F" or "R") end
  if which == "left" or which == "right" then return p.pos:sub(2, 2) == which:sub(1, 1):upper() end
  return false
end

-- args: part = door|doors|hood|trunk|all, which, state = open|close|toggle
function A.body(args)
  local part, which, state = args.part or "door", lower(args.which or ""), args.state or "toggle"
  local res = { action = "body", targets = {}, side = {}, checks = {} }
  local all, picked = BNGS.panels(), {}
  for _, p in ipairs(all) do if panelMatch(p, part, which) then table.insert(picked, p) end end
  -- The hood is a latch + catch pair: report it once, toggle both.
  -- "open the door" with no side: the front-left door.
  if part == "door" and which == "" and #picked > 1 then
    local one
    for _, p in ipairs(picked) do if p.pos == "FL" then one = p end end
    picked = { one or picked[1] }
  end
  if #picked == 0 then
    local kinds, k = {}, {}
    for _, p in ipairs(all) do if not kinds[p.kind] then kinds[p.kind] = true; table.insert(k, p.kind) end end
    res.status = "unsupported"
    res.detail = #k > 0 and ("this vehicle has: " .. table.concat(k, ", ")) or "nothing openable on this vehicle"
    return res
  end
  local changed, seen = 0, {}
  for _, p in ipairs(picked) do
    local closed = p.c.getGroupState() == "attached"
    local want = state == "toggle" and (closed and "open" or "closed") or (state == "open" and "open" or "closed")
    if (want == "closed") ~= closed then p.c.toggleGroup(); changed = changed + 1 end
    local ui = p.kind == "door" and (p.pos ~= "" and (p.pos .. " door") or p.name) or p.kind
    if not seen[ui] then
      seen[ui] = true
      table.insert(res.targets, { role = p.kind, device = p.name, ui = ui, from = closed and "closed" or "open", want = want })
      table.insert(res.checks, { src = "panel", ctrl = p.name, ui = ui, expect = want })
    end
  end
  if changed == 0 then
    for _, t in ipairs(res.targets) do t.to = t.from; t.ok = true end
    res.status = "already"; res.checks = nil; return res
  end
  res.status = "pending"
  res.verify_ms = 1800
  return res
end

---------------------------------------------------------------- engine start/stop (no key holding needed)
-- args: state = start|stop
function A.engine(args)
  local e = electrics.values
  local running = (e.engineRunning or 0) > 0.5
  local want = args.state == "stop" and "off" or "running"
  local res = { action = "engine", targets = { { role = "engine", device = "mainEngine", ui = "engine", from = running and "running" or "off", want = want } }, side = {} }
  if (want == "running") == running then res.targets[1].to = res.targets[1].from; res.targets[1].ok = true; res.status = "already"; return res end
  if want == "off" then
    electrics.setIgnitionLevel(0)
  else
    if not controller.mainController.setStarter then res.status = "unsupported"; res.detail = "no starter"; return res end
    electrics.setIgnitionLevel(2)
    controller.mainController.setStarter(true) -- released by the verify pass once the engine runs
  end
  res.status = "pending"
  res.verify_ms = 1500
  res.checks = { { src = "engine", expect = want } }
  return res
end

---------------------------------------------------------------- boost: JATO and nitrous
local function ctrlOfType(t)
  for n, c in pairs(controller.getAllControllers()) do if c.typeName == t then return n, c end end
end

-- args: state = on|off
function A.jato(args)
  if not ctrlOfType("jato") then return { action = "jato", status = "unsupported", targets = {}, detail = "no JATO fitted" } end
  local want = onoff(args.state)
  local cur = (electrics.values.jatoInput or 0) > 0.5 and 1 or 0
  local res = { action = "jato", targets = { { role = "boost", device = "jatoInput", ui = "JATO", from = cur, want = want } }, side = {} }
  if (electrics.values.jatofuel or 1) <= 0 and want == 1 then res.status = "failed"; res.detail = "JATO fuel empty (repair to refill)"; return res end
  if cur == want then res.targets[1].to = cur; res.targets[1].ok = true; res.status = "already"; return res end
  electrics.values.jatoInput = want
  res.status = "pending"
  res.checks = { { src = "e", key = "jatoInput", expect = want } }
  res.data = { fuel = electrics.values.jatofuel }
  return res
end

-- args: state = on (arm) | off (disarm). Armed nitrous injects at full throttle.
function A.nitrous(args)
  local _, c = ctrlOfType("nitrousOxideInjection")
  if not c then return { action = "nitrous", status = "unsupported", targets = {}, detail = "no nitrous fitted" } end
  local key = "nitrousOxideArm"
  local want = onoff(args.state)
  local cur = (electrics.values[key] or 0) >= 1 and 1 or 0
  local res = { action = "nitrous", targets = { { role = "boost", device = key, ui = "nitrous", from = cur, want = want } }, side = {} }
  if cur == want then res.targets[1].to = cur; res.targets[1].ok = true; res.status = "already"; return res end
  electrics.values[key] = want
  res.status = "pending"
  res.checks = { { src = "e", key = key, expect = want } }
  return res
end

---------------------------------------------------------------- trucks: engine brake, retarder, parking brake, air suspension
local CB_LEVELS = { 0.33, 0.66, 1 }

-- args: state = on|off, level = 1..3, percent
function A.engine_brake(args)
  local cb = controller.getController("compressionBrake")
  if not cb or not cb.getState then return { action = "engine_brake", status = "unsupported", targets = {}, detail = "no engine (compression) brake" } end
  local st = cb.getState() or {}
  local cur = st.compressionBrakeCoef or 0
  local level = tonumber(args.level)
  local pct = tonumber(args.percent)
  if not level and pct and pct > 0 then level = pct <= 45 and 1 or (pct <= 80 and 2 or 3) end
  local off = args.state == "off" or pct == 0
  if not level and not off then level = st.compressionBrakeLevelIndex or 3 end
  if level then level = math.max(1, math.min(3, level)) end
  local want = off and 0 or CB_LEVELS[level]
  local res = { action = "engine_brake", targets = { { role = "brake", device = "compressionBrake", ui = "engine brake", from = cur, want = want } }, side = {} }
  if math.abs(cur - want) < 0.01 then res.targets[1].to = cur; res.targets[1].ok = true; res.status = "already"; return res end
  if off then cb.setCompressionBrakeCoef(0, true)
  else cb.setCompressionBrakeLevel(level, true); cb.setCompressionBrakeCoef(want, true) end
  res.status = "pending"
  res.checks = { { src = "cbrake", expect = want } }
  return res
end

-- args: state = auto|off|on, percent = 20..100
local RET_NAMES = { "auto", "off", "20%", "40%", "60%", "80%", "100%" }
function A.retarder(args)
  local rc = controller.getController("retarderControl")
  if not rc or not rc.getState then return { action = "retarder", status = "unsupported", targets = {}, detail = "no retarder" } end
  local cur = (rc.getState() or {}).retarderControl_settingIndex
  local idx
  local pct = tonumber(args.percent)
  if pct then idx = pct <= 0 and 2 or math.max(3, math.min(7, math.floor(pct / 20 + 0.5) + 2))
  elseif args.state == "off" then idx = 2
  elseif args.state == "auto" then idx = 1
  else idx = 7 end
  local res = { action = "retarder", targets = { { role = "brake", device = "retarderControl", ui = "retarder", from = RET_NAMES[cur] or cur, want = RET_NAMES[idx] } }, side = {} }
  if cur == idx then res.targets[1].to = RET_NAMES[idx]; res.targets[1].ok = true; res.status = "already"; return res end
  if idx == 1 then rc.setRetarderMode("auto") elseif idx == 2 then rc.setRetarderMode("off") else rc.setRetarderSettingIndex(idx, true) end
  res.status = "pending"
  res.checks = { { src = "retarder", expect = idx } }
  return res
end

-- args: state = on|off
function A.parking_brake(args)
  local cur = (electrics.values.parkingbrake or 0) > 0.5 and 1 or 0
  local want = onoff(args.state)
  local res = { action = "parking_brake", targets = { { role = "brake", device = "parkingbrake", ui = "parking brake", from = cur, want = want } }, side = {} }
  if cur == want then res.targets[1].to = cur; res.targets[1].ok = true; res.status = "already"; return res end
  input.event("parkingbrake", want, FILTER_DIRECT)
  res.status = "pending"
  res.verify_ms = 800
  res.checks = { { src = "pbrake", expect = want } }
  return res
end

-- args: state = raise|lower|stop
function A.air_suspension(args)
  local as = controller.getController("airSuspension")
  if not as then return { action = "air_suspension", status = "unsupported", targets = {}, detail = "no air suspension" } end
  local s = args.state or "stop"
  -- ride-height groups live in the controller's private controlGroups table
  local groups = {}
  local i = 1
  while as.getState do
    local n, val = debug.getupvalue(as.getState, i)
    if not n then break end
    if n == "controlGroups" and type(val) == "table" then for g in pairs(val) do table.insert(groups, g) end end
    i = i + 1
  end
  if #groups == 0 then return { action = "air_suspension", status = "unsupported", detail = "no adjustable ride height on this config", targets = {} } end
  local ok, err = pcall(function()
    if s == "raise" then as.setMomentaryDecrease(groups, false); as.setMomentaryIncrease(groups, true)
    elseif s == "lower" then as.setMomentaryIncrease(groups, false); as.setMomentaryDecrease(groups, true)
    else as.stopAdjusting(groups, true) end
  end)
  if not ok then return { action = "air_suspension", status = "failed", detail = tostring(err), targets = {} } end
  return { action = "air_suspension", status = "applied", targets = { { role = "suspension", device = "airSuspension", ui = "air suspension", to = s, want = s, ok = true } }, side = {} }
end

-- Deferred verification. args: checks (from an earlier result), action, pass (1..)
function A.verify(args)
  local res = { action = args.action or "verify", targets = {}, side = {} }
  local again = false
  for _, c in ipairs(args.checks or {}) do
    local now, ok
    if c.src == "e" then now = electrics.values[c.key]
    elseif c.src == "ctrl" then local cc = controller.getController(c.ctrl); now = cc and cc.getConfig().isEnabled
    elseif c.src == "gear" then now = gearName(electrics.values.gear)
    elseif c.src == "panel" then local cc = controller.getController(c.ctrl); now = cc and (cc.getGroupState() == "attached" and "closed" or "open")
    elseif c.src == "cbrake" then now = (controller.getController("compressionBrake").getState() or {}).compressionBrakeCoef or 0
    elseif c.src == "retarder" then local i = (controller.getController("retarderControl").getState() or {}).retarderControl_settingIndex; ok = i == c.expect; now = RET_NAMES[i] or i
    elseif c.src == "pbrake" then now = (electrics.values.parkingbrake or 0) > 0.5 and 1 or 0
    elseif c.src == "engine" then
      local run = (electrics.values.engineRunning or 0) > 0.5
      now = run and "running" or "off"
      if c.expect == "running" then
        -- keep cranking for up to ~6 s (diesels), then release the starter
        if run or (args.pass or 1) >= 4 then controller.mainController.setStarter(false) else again = true end
      end
    end
    if ok == nil then
      if c.src == "gear" then ok = now ~= nil and now:sub(1, #c.expect) == c.expect
      elseif c.src == "cbrake" then ok = math.abs(now - c.expect) < 0.01
      else ok = now == c.expect end
    end
    local want = c.expect
    if c.src == "retarder" then want = RET_NAMES[c.expect] end
    table.insert(res.targets, { role = c.src, device = c.key or c.ctrl or c.src, ui = c.ui, to = now, want = want, ok = ok })
  end
  if again then res.status = "pending"; return res end
  return finish(res)
end

-- args: keys = {...} or group = temps|fuel|engine|speed|lights|assist
function A.read(args)
  local e, out = electrics.values, {}
  if args.keys then for _, k in ipairs(args.keys) do out[k] = e[k] end
  else
    local g = R.readGroups[args.group or "engine"]
    if not g then return { action = "read", status = "failed", detail = "unknown group" } end
    if g.match then for k, val in pairs(e) do if type(val) == "number" and lower(k):find(g.match) then out[k] = val end end end
    for _, k in ipairs(g.keys or {}) do out[k] = e[k] end
  end
  return { action = "read", status = "applied", data = out, targets = {} }
end

-- Inventory of everything switchable on this vehicle
function A.list(args)
  local data = { devices = {}, driveModes = {}, couplers = {}, electrics = {}, assists = {} }
  for n, c in pairs(BNGS.switchables()) do
    table.insert(data.devices, { device = n, ui = c.ui, role = c.role, cls = c.cls, mode = c.mode, modes = c.modes })
  end
  for n, cc in pairs(BNGS.driveModeCtrls()) do
    local ms = {}
    for k, m in pairs(cc.modes) do table.insert(ms, { key = k, name = m.name, order = m.order }) end
    table.sort(ms, function(a, b) return a.order < b.order end)
    table.insert(data.driveModes, { ctrl = n, current = cc.cur, modes = ms })
  end
  for _, p in ipairs(BNGS.panels()) do
    table.insert(data.couplers, { name = p.name, kind = p.kind, pos = p.pos, open = p.c.getGroupState() ~= "attached" })
  end
  local extras = {}
  for name, t in pairs({ compressionBrake = "engine brake", retarderControl = "retarder", airSuspension = "air suspension",
                         hydraulicsControl = "hydraulics" }) do
    if controller.getController(name) then table.insert(extras, t) end
  end
  if ctrlOfType("jato") then table.insert(extras, "JATO") end
  if ctrlOfType("nitrousOxideInjection") then table.insert(extras, "nitrous") end
  if electrics.values.parkingbrake ~= nil then table.insert(extras, "parking brake") end
  if controller.mainController.setStarter then table.insert(extras, "engine start/stop") end
  table.sort(extras)
  data.extras = extras
  for _, k in ipairs({ "lights_state", "fog", "hazard_enabled", "lightbar", "signal_left_input", "signal_right_input", "ignitionLevel", "parkingbrake" }) do
    if electrics.values[k] ~= nil then data.electrics[k] = electrics.values[k] end
  end
  for _, k in ipairs({ "hasABS", "hasESC", "hasTCS" }) do data.assists[k] = electrics.values[k] end
  local gb = powertrain.getDevices().gearbox
  data.gearbox = gb and { type = gb.type, gear = electrics.values.gear } or nil
  local motors = {}
  for n, d in pairs(powertrain.getDevices()) do
    if d.type == "electricMotor" or d.type == "combustionEngine" then
      local w = below(d)
      table.insert(motors, { device = n, type = d.type, propulsion = #w.wheels > 0 })
    end
  end
  data.motors = motors
  return { action = "list", status = "applied", data = data, targets = {} }
end

BNGS.actions = A

---------------------------------------------------------------- entry
function BNGS.call(action, args, rules, tag)
  mergeRules(rules)
  local fn = A[action]
  local ok, res
  if not fn then ok, res = true, { action = action, status = "unsupported", detail = "unknown action" }
  else ok, res = pcall(fn, args or {}) end
  if not ok then res = { action = action, status = "failed", detail = tostring(res) } end
  if res.steps then for _, s in ipairs(res.steps) do s.c = nil end end
  return (tag or "") .. jsonEncode(res)
end
