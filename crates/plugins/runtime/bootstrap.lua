assert(type(plugin) == 'table', 'plugin must return a table')
-- The host reads the returned table through the C API; nothing is encoded here.
function __nana_dispatch(event)
  if event.type == 'data' then
    local results = {}
    for _, r in ipairs(event.results or {}) do
      if __nana_data.pending[r.token] then
        __nana_data.pending[r.token] = nil
        results[#results + 1] = r
      end
    end
    event = {type = 'data', results = results}
  else
    -- Requests left unanswered by the previous host event are discarded.
    __nana_data.pending = {}
  end
  if plugin.on_event then plugin.on_event(event) end
  local update = plugin.view and plugin.view() or {}
  local requests = __nana_data.outbox
  __nana_data.outbox = {}
  return {update = update, requests = requests}
end
return ''
