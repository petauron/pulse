export const AUTH_REQUIRED_EVENT = 'pulse:auth-required'
const pathSuffix = /[?#]/
let sessionGeneration = 0

export function getSessionGeneration(): number {
  return sessionGeneration
}

export function invalidateSessionRequests(): void {
  sessionGeneration += 1
}

export function requireLogin(generation = sessionGeneration): void {
  if (generation === sessionGeneration)
    window.dispatchEvent(new Event(AUTH_REQUIRED_EVENT))
}

/** Only same-origin application paths may be used after authentication. */
export function loginDestination(value: unknown): string {
  if (typeof value !== 'string' || !value.startsWith('/') || value.startsWith('//') || value.includes('\\'))
    return '/admin'
  const path = value.split(pathSuffix)[0]
  return path === '/login' ? '/admin' : value
}
