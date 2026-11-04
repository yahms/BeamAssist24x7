-- Vehicle VM. Dumps powertrain devices, controllers, and electrics.
-- Wrap with pcall + tag when sending via run_lua_vehicle.
local function wheelsBelow(d,acc) acc=acc or {} if d.connectedWheel then table.insert(acc,d.connectedWheel) end for _,c in ipairs(d.children or {}) do wheelsBelow(c,acc) end return acc end
local devs={}
for name,d in pairs(powertrain.getDevices()) do
  local am={} for _,m in ipairs(d.availableModes or {}) do table.insert(am,m) end
  local kids={} for _,c in ipairs(d.children or {}) do table.insert(kids,c.name) end
  local cats={} for k in pairs(d.deviceCategories or {}) do table.insert(cats,k) end
  devs[name]={type=d.type,ui=d.uiName,mode=d.mode,modes=am,def=d.defaultMode,parent=d.parent and d.parent.name,kids=kids,wheels=wheelsBelow(d),cats=cats,setMode=d.setMode~=nil,ratio=d.gearRatio,broken=d.isBroken,gi=d.gearIndex,gcount=d.gearCount}
end
local ctrls={}
for n,c in pairs(controller.getAllControllers()) do
  local f={} for k,v in pairs(c) do if type(v)=='function' and (k:find('^set') or k:find('^toggle') or k:find('^get') or k:find('Mode')) then table.insert(f,k) end end
  table.sort(f) ctrls[n]={type=c.typeName,funcs=f}
end
local ev={} for k,v in pairs(electrics.values) do if type(v)=='number' or type(v)=='boolean' then ev[k]=v end end
local ef={} for k,v in pairs(electrics) do if type(v)=='function' and (k:find('^set') or k:find('^toggle') or k:find('^get')) then table.insert(ef,k) end end table.sort(ef)
return {devices=devs,controllers=ctrls,electricsFuncs=ef,electrics=ev}
