import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError, PulseApi, setCsrfToken } from '../api'
import { invalidateSessionRequests } from '../session'

const fetchMock = vi.fn()
const dispatchEvent = vi.fn()

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock)
  vi.stubGlobal('window', { location: { origin: 'https://pulse.example' }, setTimeout, clearTimeout, dispatchEvent })
  setCsrfToken('test-csrf')
})

afterEach(() => {
  vi.unstubAllGlobals()
  fetchMock.mockReset()
  dispatchEvent.mockReset()
  setCsrfToken('')
})

describe('authenticated API transport', () => {
  it('shows the nested Service error message without object coercion', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ error: { message: 'invalid node metadata' } }), { status: 400 }))
    await expect(new PulseApi('/api').post('admin/nodes/test', {})).rejects.toThrow('invalid node metadata')
  })

  it('stops oversized chunked responses even without content-length', async () => {
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new Uint8Array(2 * 1024 * 1024 + 1))
        controller.close()
      },
    })
    fetchMock.mockResolvedValue(new Response(stream))
    await expect(new PulseApi('/api').get('public')).rejects.toThrow('safety limit')
  })

  it('preserves same-origin credentials and CSRF on a mutation', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ ok: true })))
    await new PulseApi('/api').post('admin/settings', { private_site: true })
    expect(fetchMock).toHaveBeenCalledWith(new URL('https://pulse.example/api/admin/settings'), expect.objectContaining({
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': 'test-csrf' },
      body: JSON.stringify({ private_site: true }),
    }))
  })

  it('signals expired management authentication once without retrying', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ error: 'Unauthorized' }), { status: 401 }))
    await expect(new PulseApi('/api').get('admin/state')).rejects.toBeInstanceOf(ApiError)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(dispatchEvent).toHaveBeenCalledTimes(1)
    expect(dispatchEvent.mock.calls[0]![0].type).toBe('pulse:auth-required')
  })

  it('keeps invalid login credentials on the login form instead of redirecting', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ error: 'Invalid credentials' }), { status: 401 }))
    await expect(new PulseApi('/api').post('auth/login', { username: 'admin', password: 'incorrect' })).rejects.toThrow('Invalid credentials')
    expect(dispatchEvent).not.toHaveBeenCalled()
  })

  it('does not clear a newer login when an old request later returns 401', async () => {
    let resolveResponse!: (response: Response) => void
    fetchMock.mockReturnValue(new Promise<Response>((resolve) => {
      resolveResponse = resolve
    }))
    const pending = new PulseApi('/api').get('admin/state')
    invalidateSessionRequests()
    resolveResponse(new Response(JSON.stringify({ error: 'Expired old session' }), { status: 401 }))
    await expect(pending).rejects.toBeInstanceOf(ApiError)
    expect(dispatchEvent).not.toHaveBeenCalled()
  })
})
