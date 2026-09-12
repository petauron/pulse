import type { KomariRpc } from '@/utils/rpc'
import { useAppStore } from '@/stores/app'
import { useAuthStore } from '@/stores/auth'
import { useNodesStore } from '@/stores/nodes'
import { ApiError, getSharedApi } from '@/utils/api'
import { getSharedRpc, RpcError } from '@/utils/rpc'

class InitManager {
  private readonly rpc: KomariRpc
  private readonly appStore: ReturnType<typeof useAppStore>
  private readonly nodesStore: ReturnType<typeof useNodesStore>
  private pollTimer: ReturnType<typeof setTimeout> | null = null
  private isPolling = false
  private isInitialized = false
  private isDestroyed = false

  constructor() {
    this.rpc = getSharedRpc()
    this.appStore = useAppStore()
    this.nodesStore = useNodesStore()
  }

  private getPollInterval(): number {
    const interval = this.appStore.publicSettings?.theme_settings?.dataUpdateInterval
    return typeof interval === 'number' && interval >= 1 && interval <= 300
      ? interval * 1000
      : 3000
  }

  async init(): Promise<void> {
    if (this.isInitialized)
      return
    await this.poll()
  }

  private async poll(): Promise<void> {
    if (this.isPolling || this.isDestroyed)
      return
    this.isPolling = true
    try {
      if (!this.isInitialized) {
        const api = getSharedApi()
        // Wait for every bounded request before retrying, even when one fails
        // immediately, so repeated failures cannot accumulate pending requests.
        const [publicSettings, dashboard] = await Promise.allSettled([
          api.getPublicSettings(),
          this.rpc.getDashboard(),
        ])
        if (this.isDestroyed)
          return
        if (publicSettings.status === 'rejected')
          throw publicSettings.reason
        if (dashboard.status === 'rejected')
          throw dashboard.reason
        this.appStore.publicSettings = publicSettings.value
        this.applyDashboard(dashboard.value)
        this.isInitialized = true
      }
      else {
        const dashboard = await this.rpc.getDashboard()
        if (this.isDestroyed)
          return
        this.applyDashboard(dashboard)
      }
      this.appStore.connectionError = false
    }
    catch (error) {
      if (this.isDestroyed)
        return
      if ((error instanceof RpcError && error.code === 401) || (error instanceof ApiError && error.statusCode === 401)) {
        this.destroy()
        this.appStore.loading = false
        return
      }
      const message = error instanceof RpcError ? error.message : String(error)
      console.warn('[Pulse] Connection failed; retrying:', message)
      this.appStore.connectionError = true
    }
    finally {
      this.isPolling = false
      if (!this.isDestroyed) {
        this.appStore.loading = false
        this.stopPolling()
        this.pollTimer = setTimeout(() => void this.poll(), this.getPollInterval())
      }
    }
  }

  private applyDashboard(dashboard: Awaited<ReturnType<KomariRpc['getDashboard']>>): void {
    // The Service enforces visibility; also discard hidden data from guest UI state.
    const clients = useAuthStore().loggedIn
      ? dashboard.clients
      : Object.fromEntries(Object.entries(dashboard.clients).filter(([, client]) => !client.hidden))
    this.nodesStore.initNodes(clients, dashboard.statuses)
  }

  stopPolling(): void {
    if (this.pollTimer) {
      clearTimeout(this.pollTimer)
      this.pollTimer = null
    }
  }

  destroy(): void {
    this.isDestroyed = true
    this.stopPolling()
    this.rpc.close()
    this.nodesStore.clearNodes()
    this.isInitialized = false
  }
}

let initManager: InitManager | null = null

export async function initApp(): Promise<void> {
  initManager ??= new InitManager()
  await initManager.init()
}

export function getInitManager(): InitManager | null {
  return initManager
}

export function destroyInitManager(): void {
  initManager?.destroy()
  initManager = null
}
