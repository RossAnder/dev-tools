// TEST-BOOTSTRAP:STUB
// Smoke test — confirms `bun test` is wired correctly for the Vue SPA target.
// Should pass on first run. Invoke: `bun test` (from lumina/web/).

import { describe, expect, test } from 'bun:test'

describe('smoke', () => {
  test('arithmetic works', () => {
    expect(1 + 1).toBe(2)
  })

  test('async resolution works', async () => {
    const val = await Promise.resolve(42)
    expect(val).toBe(42)
  })
})
