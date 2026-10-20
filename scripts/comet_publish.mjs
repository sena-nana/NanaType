// Stand in for the 彗星号 page: push integrations/comet/fixtures.json through
// the page's own converter to NanaType's relay, twice, so the second round
// shows that repeated messages are dropped.
//
//   node scripts/comet_publish.mjs "ws://127.0.0.1:12451/comet?token=…"
//
// Needs Node 22 or later (global WebSocket).
import { readFileSync } from 'node:fs'
import vm from 'node:vm'

const address = process.argv[2]
if (!address) {
  console.error('usage: node scripts/comet_publish.mjs <address copied from NanaType>')
  process.exit(2)
}
const dir = new URL('../integrations/comet/', import.meta.url)
const html = readFileSync(new URL('index.html', dir), 'utf8')
const context = vm.createContext({})
vm.runInContext(html.match(/<script id="convert">([\s\S]*?)<\/script>/)[1], context)
const fixtures = JSON.parse(readFileSync(new URL('fixtures.json', dir), 'utf8'))
const now = Math.floor(Date.now() / 1000)
const frames = fixtures
  .map(f => context.toBlivechat({ ...f.comet, timestamp: now }))
  .filter(Boolean)

const ws = new WebSocket(address)
ws.onerror = () => {
  console.error('cannot connect: is 彗星号 on in NanaType, and the token right?')
  process.exit(1)
}
ws.onopen = async () => {
  for (const round of [1, 2]) {
    for (const frame of frames) {
      ws.send(JSON.stringify(frame))
      await new Promise(r => setTimeout(r, 400))
    }
    console.log(`round ${round}: sent ${frames.length} frames`)
  }
  ws.onerror = null
  ws.close()
}
