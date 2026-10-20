// node --test integrations/comet/
// Runs the page's own converter (the #convert script) over fixtures.json.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import vm from 'node:vm'

const here = new URL('.', import.meta.url)
const html = readFileSync(new URL('index.html', here), 'utf8')
const source = html.match(/<script id="convert">([\s\S]*?)<\/script>/)[1]
const context = vm.createContext({})
vm.runInContext(source, context)
const fixtures = JSON.parse(readFileSync(new URL('fixtures.json', here), 'utf8'))

for (const fixture of fixtures) {
  test(fixture.name, () => {
    const frame = context.toBlivechat(fixture.comet)
    assert.deepEqual(frame === null ? null : JSON.parse(JSON.stringify(frame)), fixture.blivechat)
  })
}

test('nothing usable gives no frame', () => {
  assert.equal(context.toBlivechat(null), null)
  assert.equal(context.toBlivechat({ type: 'like', likeCount: 3 }), null)
})

test('a message without an id gets a stable one', () => {
  const msg = { type: 'message', uid: '1', timestamp: 5, message: 'hi', platform: 'bili' }
  assert.equal(context.toBlivechat(msg).data[11], context.toBlivechat(msg).data[11])
  assert.notEqual(context.toBlivechat(msg).data[11], '')
})
