import { getSessionGeneration, requireLogin } from './session'

const DEFAULT_API_BASE = '/api'
const DEFAULT_TIMEOUT_MS = 15_000
const MAX_RESPONSE_BYTES = 2 * 1024 * 1024
// Administration can contain up to 1000 nodes and 128 bounded assignment lists.
const MAX_ADMIN_RESPONSE_BYTES = 32 * 1024 * 1024
let csrfToken = ''

export function setCsrfToken(value: string): void {
  csrfToken = value
}

interface ApiResponse<T> {
  status: 'success' | 'error'
  message: string
  data: T
}

export interface PublicSettings {
  allow_cors: boolean
  custom_body: string
  custom_head: string
  description: string
  disable_password_login: boolean
  oauth_enable: boolean
  oauth_provider: string | null
  ping_record_preserve_time: number
  private_site: boolean
  record_enabled: boolean
  record_preserve_time: number
  sitename: string
  theme: string
  theme_settings?: Record<string, unknown> | null
}

export interface VersionInfo {
  hash: string
  version: string
}

export class ApiError extends Error {
  readonly statusCode?: number

  constructor(message: string, statusCode?: number) {
    super(message)
    this.name = 'ApiError'
    this.statusCode = statusCode
  }
}

export class PulseApi {
  private readonly baseUrl: URL

  constructor(baseUrl = import.meta.env.VITE_API_BASE || DEFAULT_API_BASE) {
    this.baseUrl = new URL(baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`, window.location.origin)
    if (this.baseUrl.origin !== window.location.origin)
      throw new ApiError('Pulse Web API must be served from the same origin')
  }

  private async request<T>(path: string, wrapped: boolean, body?: unknown, signal?: AbortSignal): Promise<T> {
    const generation = getSessionGeneration()
    const controller = new AbortController()
    const timer = window.setTimeout(() => controller.abort(), DEFAULT_TIMEOUT_MS)
    const cancel = () => controller.abort()
    if (signal?.aborted)
      controller.abort()
    signal?.addEventListener('abort', cancel, { once: true })
    try {
      const response = await fetch(new URL(path, this.baseUrl), {
        cache: 'no-store',
        method: body === undefined ? 'GET' : 'POST',
        headers: body === undefined ? undefined : { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken },
        body: body === undefined ? undefined : JSON.stringify(body),
        credentials: 'same-origin',
        signal: controller.signal,
      })

      const maximumBytes = path === 'admin/state' ? MAX_ADMIN_RESPONSE_BYTES : MAX_RESPONSE_BYTES
      const contentLength = Number(response.headers.get('content-length'))
      if (Number.isFinite(contentLength) && contentLength > maximumBytes) {
        controller.abort()
        throw new ApiError('API response exceeded the client safety limit')
      }
      let text = ''
      let bytes = 0
      const reader = response.body?.getReader()
      if (reader) {
        const decoder = new TextDecoder()
        try {
          while (true) {
            const chunk = await reader.read()
            if (chunk.done)
              break
            bytes += chunk.value.byteLength
            if (bytes > maximumBytes) {
              controller.abort()
              throw new ApiError('API response exceeded the client safety limit')
            }
            text += decoder.decode(chunk.value, { stream: true })
          }
          text += decoder.decode()
        }
        finally { reader.releaseLock() }
      }

      let payload: unknown
      try {
        payload = text ? JSON.parse(text) : null
      }
      catch {
        throw new ApiError(`服务返回了无法读取的响应（HTTP ${response.status}）`, response.status)
      }
      if (!response.ok) {
        if (response.status === 401 && !path.startsWith('auth/'))
          requireLogin(generation)
        const failure = payload as { error?: string | { message?: string }, message?: string } | null
        const message = typeof failure?.error === 'string' ? failure.error : failure?.error?.message
        throw new ApiError(message || failure?.message || `请求失败（HTTP ${response.status}）`, response.status)
      }
      if (!wrapped)
        return payload as T

      const result = payload as ApiResponse<T>
      if (result.status !== 'success')
        throw new ApiError(result.message || 'API request failed', response.status)
      return result.data
    }
    catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError')
        throw new ApiError('API request timed out')
      if (error instanceof ApiError)
        throw error
      throw new ApiError(error instanceof Error ? error.message : String(error))
    }
    finally {
      window.clearTimeout(timer)
      signal?.removeEventListener('abort', cancel)
    }
  }

  get<T>(path: string, signal?: AbortSignal): Promise<T> {
    return this.request<T>(path, false, undefined, signal)
  }

  post<T>(path: string, body: unknown = {}): Promise<T> {
    return this.request<T>(path, false, body)
  }

  getPublicSettings(): Promise<PublicSettings> {
    return this.request<PublicSettings>('public', true)
  }

  getVersion(): Promise<VersionInfo> {
    return this.request<VersionInfo>('version', true)
  }
}

let sharedApiInstance: PulseApi | null = null

export function getSharedApi(): PulseApi {
  sharedApiInstance ??= new PulseApi()
  return sharedApiInstance
}

export function resetSharedApi(): void {
  sharedApiInstance = null
}
