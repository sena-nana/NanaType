-- Defined before the plugin chunk runs. Every limit here is checked again by the host.
__nana_data = {pending = {}, outbox = {}, next = 0}
nana = {data = {}}
function nana.data.request(id, query)
  assert(type(id) == 'string' and #id > 0 and #id <= 128, 'data ID must be a short string')
  assert(type(query) == 'table', 'data query must be a table')
  local count = 0
  for _ in pairs(__nana_data.pending) do count = count + 1 end
  assert(count < 8, 'at most 8 pending data requests')
  __nana_data.next = __nana_data.next + 1
  local token = __nana_data.next
  __nana_data.pending[token] = true
  __nana_data.outbox[#__nana_data.outbox + 1] = {token = token, id = id, query = query}
  return token
end
