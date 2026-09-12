<script setup lang="ts">
import type { AdminState, SiteSettings } from '@/utils/admin'
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AccountSecurity from '@/components/admin/AccountSecurity.vue'
import AdminAlerts from '@/components/admin/AdminAlerts.vue'
import AdminNodes from '@/components/admin/AdminNodes.vue'
import AdminProbes from '@/components/admin/AdminProbes.vue'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Input } from '@/components/ui/input'
import { useAdminAction } from '@/composables/useAdminAction'
import { useAppStore } from '@/stores/app'
import { useAuthStore } from '@/stores/auth'
import { getSharedApi } from '@/utils/api'
import { destroyInitManager } from '@/utils/init'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const app = useAppStore()
const state = shallowRef<AdminState | null>(null)
const settings = ref<SiteSettings>({ site_name: 'Pulse', private_site: true, agent_interval_seconds: 3 })
const loading = ref(false)
const loadError = ref('')
let controller: AbortController | null = null
let disposed = false
const sections = [{ id: 'nodes', label: '节点' }, { id: 'probes', label: '探测' }, { id: 'alerts', label: '通知与告警' }, { id: 'site', label: '站点设置' }, { id: 'security', label: '账号安全' }]
const section = computed(() => sections.some(item => item.id === route.query.section) ? route.query.section : 'nodes')

async function refresh(): Promise<void> {
  if (!auth.loggedIn || loading.value)
    return
  loading.value = true
  loadError.value = ''
  controller = new AbortController()
  try {
    const response = await getSharedApi().get<AdminState>('admin/state', controller.signal)
    if (!disposed) {
      state.value = response
      settings.value = { ...response.settings }
      if (app.publicSettings)
        app.publicSettings = { ...app.publicSettings, sitename: response.settings.site_name, private_site: response.settings.private_site, theme_settings: { ...app.publicSettings.theme_settings, dataUpdateInterval: response.settings.agent_interval_seconds } }
    }
  }
  catch (cause) {
    if (!disposed)
      loadError.value = cause instanceof Error ? cause.message : String(cause)
    throw cause
  }
  finally { loading.value = false }
}

const { busy, error, success, run } = useAdminAction(refresh)
onMounted(() => void refresh().catch(() => {}))
onBeforeUnmount(() => {
  disposed = true
  controller?.abort()
  state.value = null
})

async function saveSettings(): Promise<void> {
  await run(async () => {
    await getSharedApi().post('admin/settings', settings.value)
  })
}

async function logout(): Promise<void> {
  await run(async () => {
    await auth.logout()
    destroyInitManager()
    state.value = null
    await router.replace('/login')
  }, '', false)
}
</script>

<template>
  <div class="space-y-4 px-4 pb-8">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-xl font-semibold">
          管理 Pulse
        </h1><p class="text-sm text-muted-foreground">
          {{ auth.status?.username }}
        </p>
      </div><div class="flex gap-2">
        <Button variant="outline" :disabled="loading || busy" @click="refresh().catch(() => {})">
          {{ loading ? '正在刷新…' : '刷新' }}
        </Button><Button variant="outline" :disabled="busy" @click="logout">
          退出登录
        </Button>
      </div>
    </div>
    <nav aria-label="管理页面" class="flex flex-wrap gap-2">
      <RouterLink v-for="item in sections" :key="item.id" :to="{ name: 'admin', query: { section: item.id } }" :aria-current="section === item.id ? 'page' : undefined" class="inline-flex min-h-11 items-center rounded-md px-3 text-sm transition-colors focus-visible:ring-2 focus-visible:ring-ring" :class="section === item.id ? 'bg-primary text-primary-foreground' : 'bg-background/60 hover:bg-accent'">
        {{ item.label }}
      </RouterLink>
    </nav>
    <p v-if="loadError" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
      {{ loadError }}
    </p>
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p><p v-if="success" role="status" class="text-sm text-success">
      {{ success }}
    </p>
    <p v-if="!state && loading" role="status" class="text-sm text-muted-foreground">
      正在读取管理数据…
    </p>
    <template v-if="state">
      <AdminNodes v-if="section === 'nodes'" :nodes="state.nodes" :refresh="refresh" />
      <AdminProbes v-else-if="section === 'probes'" :nodes="state.nodes" :probes="state.probes" :refresh="refresh" />
      <AdminAlerts v-else-if="section === 'alerts'" :nodes="state.nodes" :channels="state.channels" :rules="state.alert_rules" :incidents="state.incidents" :failures="state.notification_failures" :refresh="refresh" />
      <AccountSecurity v-else-if="section === 'security'" />
      <CardX v-else title="站点设置" content-class="space-y-4">
        <form class="max-w-xl space-y-4" :aria-busy="busy" @submit.prevent="saveSettings">
          <fieldset :disabled="busy" class="space-y-4">
            <label class="grid gap-2 text-sm">站点名称<Input v-model="settings.site_name" required maxlength="128" /></label>
            <label class="grid gap-2 text-sm">Agent 上报间隔（秒）<Input v-model="settings.agent_interval_seconds" type="number" min="1" max="300" step="1" required /></label>
            <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="settings.private_site" type="checkbox" class="size-4 accent-primary">私有站点：登录后才能查看监控数据</label>
            <p class="text-sm text-muted-foreground">
              关闭私有站点后，未隐藏节点的监控数据对访客公开。
            </p>
          </fieldset>
          <Button type="submit" class="min-h-11" :disabled="busy">
            {{ busy ? '正在保存…' : '保存设置' }}
          </Button>
        </form>
      </CardX>
    </template>
  </div>
</template>
