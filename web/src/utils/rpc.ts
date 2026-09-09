const DEFAULT_RPC_API_BASE = '/api'
const DEFAULT_TIMEOUT_MS = 15_000
const MAX_RESPONSE_CHARACTERS = 2 * 1024 * 1024

interface JsonRpcResponse<T> {
  jsonrpc: '2.0'
  result?: T
  error?: {
    code: number
    message: string
    data?: unknown
  }
  id: number
}

export interface Client {
  uuid: string
  name: string
  cpu_name: string
  virtualization: string
  arch: string
  cpu_cores: number
  os: string
  kernel_version: string
  gpu_name?: string
  ipv4?: string
  ipv6?: string
  region: string
  remark?: string
  public_remark: string
  mem_total: number
  swap_total: number
  disk_total: number
  version?: string
  weight: number
  price: number
  billing_cycle: number
  auto_renewal: boolean
  currency: string
  expired_at: string
  group: string
  tags: string
  hidden: boolean
  traffic_limit: number
  traffic_limit_type: string
  created_at: string
  updated_at: string
}

export interface NodeStatus {
  client: string
  time: string
  collected_time?: string
  clock_skew_ms?: number
  cpu: number
  gpu: number | null
  ram: number
  ram_total: number
  swap: number
  swap_total: number
  load: number
  load5: number
  load15: number
  temp: number | null
  disk: number
  disk_total: number
  net_in: number
  net_out: number
  net_total_up: number
  net_total_down: number
  process: number | null
  connections: number | null
  connections_udp: number | null
  online: boolean
  uptime: number
}

export interface HistoryCoverage {
  requested_start_unix_ms: number
  requested_end_unix_ms: number
  actual_start_unix_ms: number | null
  actual_end_unix_ms: number | null
  source_points: number
  returned_points: number
  bucket_ms: number
  downsampled: boolean
}

export type StatusRecord = Omit<NodeStatus, 'online' | 'uptime'>

export interface HistoryResponse {
  count: number
  records: StatusRecord[]
  coverage: HistoryCoverage
}

export interface DashboardResponse {
  clients: Record<string, Client>
  statuses: Record<string, NodeStatus>
}

export interface PublicInfo {
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
  theme_settings: Record<string, unknown>
}

export interface VersionInfo {
  version: string
  hash: string
}

export class RpcError extends Error {
  code: number
  data?: unknown

  constructor(code: number, message: string, data?: unknown) {
    super(message)
    this.name = 'RpcError'
    this.code = code
    this.data = data
  }
}

export class RpcClient {
  private readonly endpoint: string
  private requestId = 0

  constructor(baseUrl = `${import.meta.env.VITE_API_BASE || DEFAULT_RPC_API_BASE}/rpc2`) {
    this.endpoint = new URL(baseUrl, window.location.origin).toString()
  }

  async call<T>(
    method: string,
    params: Record<string, unknown> = {},
    signal?: AbortSignal,
  ): Promise<T> {
    const controller = new AbortController()
    let timedOut = false
    const timer = window.setTimeout(() => {
      timedOut = true
      controller.abort()
    }, DEFAULT_TIMEOUT_MS)
    const cancelFromCaller = () => controller.abort()
    if (signal?.aborted)
      controller.abort()
    else
      signal?.addEventListener('abort', cancelFromCaller, { once: true })
    const id = ++this.requestId
    try {
      const response = await fetch(this.endpoint, {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', method, params, id }),
        signal: controller.signal,
      })
      if (!response.ok)
        throw new RpcError(response.status, `RPC request failed with HTTP ${response.status}`)

      const text = await response.text()
      if (text.length > MAX_RESPONSE_CHARACTERS)
        throw new RpcError(-32001, 'RPC response exceeded the client safety limit')
      const payload = JSON.parse(text) as JsonRpcResponse<T>
      if (payload.id !== id || payload.jsonrpc !== '2.0')
        throw new RpcError(-32603, 'Invalid RPC response')
      if (payload.error)
        throw new RpcError(payload.error.code, payload.error.message, payload.error.data)
      if (payload.result === undefined)
        throw new RpcError(-32603, 'RPC response did not contain a result')
      return payload.result
    }
    catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') {
        throw new RpcError(
          timedOut ? -32000 : -32800,
          timedOut ? 'RPC request timed out' : 'RPC request cancelled',
        )
      }
      throw error
    }
    finally {
      window.clearTimeout(timer)
      signal?.removeEventListener('abort', cancelFromCaller)
    }
  }
}

export class KomariRpc {
  private readonly client = new RpcClient()

  getClient(): RpcClient {
    return this.client
  }

  getDashboard(): Promise<DashboardResponse> {
    return this.client.call<DashboardResponse>('common:getDashboard')
  }

  getNodeRecentStatus(
    uuid: string,
    limit = 150,
    signal?: AbortSignal,
  ): Promise<HistoryResponse> {
    return this.client.call<HistoryResponse>('common:getNodeRecentStatus', { uuid, limit }, signal)
  }

  getLoadRecords(
    uuid: string,
    hours: number,
    maxCount = 1_000,
    signal?: AbortSignal,
  ): Promise<HistoryResponse> {
    return this.client.call<HistoryResponse>('common:getRecords', {
      type: 'load',
      uuid,
      hours,
      max_count: maxCount,
    }, signal)
  }

  getPublicInfo(): Promise<PublicInfo> {
    return this.client.call<PublicInfo>('common:getPublicInfo')
  }

  getBackendVersion(): Promise<VersionInfo> {
    return this.client.call<VersionInfo>('common:getBackendVersion')
  }

  close(): void {}
}

let sharedRpc: KomariRpc | null = null

export function getSharedRpc(): KomariRpc {
  sharedRpc ??= new KomariRpc()
  return sharedRpc
}

export function resetSharedRpc(): void {
  sharedRpc = null
}
