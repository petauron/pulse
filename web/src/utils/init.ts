import type { KomariRpc } from '@/utils/rpc'
import { useAppStore } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { getSharedApi } from '@/utils/api'
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
    return typeof interval === 'number' && interval >= 1 && interval <= 60
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
        const [publicSettings, userInfo, dashboard] = await Promise.allSettled([
          api.getPublicSettings(),
          api.getMe(),
          this.rpc.getDashboard(),
        ])
        if (this.isDestroyed)
          return
        if (publicSettings.status === 'rejected')
          throw publicSettings.reason
        if (userInfo.status === 'rejected')
          throw userInfo.reason
        if (dashboard.status === 'rejected')
          throw dashboard.reason
        this.appStore.publicSettings = publicSettings.value
        this.appStore.updateLoginState(userInfo.value.logged_in)
        this.nodesStore.initNodes(dashboard.value.clients, dashboard.value.statuses)
        this.isInitialized = true
      }
      else {
        const dashboard = await this.rpc.getDashboard()
        if (this.isDestroyed)
          return
        this.nodesStore.initNodes(dashboard.clients, dashboard.statuses)
      }
      this.appStore.connectionError = false
    }
    catch (error) {
      if (this.isDestroyed)
        return
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
