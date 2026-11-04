-- Vehicle VM. For every driveModes controller, list modes and what each mode sets.
local out={}
for n,c in pairs(controller.getAllControllers()) do
  if c.typeName=='driveModes' then
    local cfg=v.data[n]
    local keys={}
    if cfg and cfg.modes then for k in pairs(cfg.modes) do table.insert(keys,k) end end
    local modes={}
    for _,k in ipairs(keys) do
      local ok,d=pcall(c.getDriveModeData,k)
      if ok and d then
        local s={}
        for sk,st in pairs(d.settings or {}) do
          if st.type=='powertrainDeviceMode' then table.insert(s,st.deviceName..'='..st.mode)
          elseif st.type=='electricsValue' then table.insert(s,'e.'..st.electricsName..'='..tostring(st.value))
          elseif st.type and st.type~='simpleControlButton' and st.type~='quickAccess' then table.insert(s,st.type..':'..(st.name or st.controllerName or sk)) end
        end
        table.sort(s)
        modes[k]={name=d.name,order=d.order,set=s}
      end
    end
    out[n]={current=c.getCurrentDriveModeKey(),modes=modes}
  end
end
return out
