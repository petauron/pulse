const DEFAULT_API_BASE = '/api'
const DEFAULT_TIMEOUT_MS = 15_000
const MAX_RESPONSE_CHARACTERS = 256 * 1024

interface ApiResponse<T> {
  status: 'success' | 'error'
  message: string
  data: T
}

export interface MeInfo {
  logged_in: boolean
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

  private async request<T>(path: string, wrapped: boolean): Promise<T> {
    const controller = new AbortController()
    const timer = window.setTimeout(() => controller.abort(), DEFAULT_TIMEOUT_MS)
    try {
      const response = await fetch(new URL(path, this.baseUrl), {
        credentials: 'same-origin',
        signal: controller.signal,
      })
      if (!response.ok)
        throw new ApiError(`API request failed with HTTP ${response.status}`, response.status)

      const contentLength = Number(response.headers.get('content-length'))
      if (Number.isFinite(contentLength) && contentLength > MAX_RESPONSE_CHARACTERS)
        throw new ApiError('API response exceeded the client safety limit')

      const text = await response.text()
      if (text.length > MAX_RESPONSE_CHARACTERS)
        throw new ApiError('API response exceeded the client safety limit')

      const payload: unknown = JSON.parse(text)
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
    }
  }

  getMe(): Promise<MeInfo> {
    return this.request<MeInfo>('me', false)
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
