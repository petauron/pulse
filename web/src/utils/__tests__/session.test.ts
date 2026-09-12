import { describe, expect, it } from 'vitest'
import { loginDestination } from '../session'

describe('post-login destination', () => {
  it('preserves local admin and node deep links', () => {
    expect(loginDestination('/admin?section=probes')).toBe('/admin?section=probes')
    expect(loginDestination('/instance/node-1')).toBe('/instance/node-1')
  })

  it('rejects external, protocol-relative and backslash destinations', () => {
    for (const value of ['https://example.org', '//example.org', '/\\example.org', undefined, ['/', '/admin']])
      expect(loginDestination(value)).toBe('/admin')
  })

  it('avoids redirecting back to login', () => {
    expect(loginDestination('/login?redirect=/login')).toBe('/admin')
  })
})
