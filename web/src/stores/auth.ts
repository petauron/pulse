import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { getSharedApi, setCsrfToken } from '@/utils/api'
import { invalidateSessionRequests } from '@/utils/session'
import { useAppStore } from './app'
import { useNodesStore } from './nodes'

export interface AuthStatus {
  initialized: boolean
  logged_in: boolean
  username: string | null
  csrf_token: string
  totp_enabled: boolean
  oauth_enabled: boolean
  oauth_totp_required: boolean
}

export const useAuthStore = defineStore('auth', () => {
  const status = ref<AuthStatus | null>(null)
  const error = ref('')
  const epoch = ref(0)
  const loggedIn = computed(() => status.value?.logged_in === true)
  let pending: Promise<AuthStatus> | null = null

  function accept(value: AuthStatus): AuthStatus {
    status.value = value
    setCsrfToken(value.csrf_token)
    useAppStore().updateLoginState(value.logged_in)
    error.value = ''
    return value
  }

  async function refresh(): Promise<AuthStatus> {
    if (pending)
      return pending
    pending = getSharedApi().get<AuthStatus>('auth/status').then(accept).catch((cause) => {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    }).finally(() => { pending = null })
    return pending
  }

  function clear(): void {
    invalidateSessionRequests()
    status.value = null
    setCsrfToken('')
    useAppStore().updateLoginState(false)
    useNodesStore().clearNodes()
    epoch.value += 1
  }

  async function authenticate(path: 'setup' | 'login' | 'oauth/complete', body: unknown): Promise<void> {
    accept(await getSharedApi().post<AuthStatus>(`auth/${path}`, body))
    invalidateSessionRequests()
    useNodesStore().clearNodes()
    epoch.value += 1
  }

  async function logout(): Promise<void> {
    await getSharedApi().post('auth/logout')
    clear()
  }

  return { status, error, epoch, loggedIn, refresh, clear, authenticate, logout }
})
