<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Button } from '@/components/ui/button'
import { getSharedApi } from '@/utils/api'

interface ThemeManifest {
  name: string | Record<string, string>
  short: string
  version: string
  description?: string | Record<string, string>
  author?: string | Record<string, string>
  configuration?: unknown
}

interface ThemeListing {
  active: string
  themes: ThemeManifest[]
}

const listing = ref<ThemeListing | null>(null)
const selectedFile = ref<File | null>(null)
const trusted = ref(false)
const busy = ref(false)
const error = ref('')
const success = ref('')
const settingsDraft = ref('{}')

function localized(value: string | Record<string, string> | undefined): string {
  if (!value)
    return ''
  if (typeof value === 'string')
    return value
  return value['zh-CN'] || value.en || Object.values(value)[0] || ''
}

async function refresh(): Promise<void> {
  const response = await getSharedApi().get<{ data: ThemeListing }>('admin/theme/list')
  listing.value = response.data
  const publicSettings = await getSharedApi().getPublicSettings()
  settingsDraft.value = JSON.stringify(publicSettings.theme_settings ?? {}, null, 2)
}

async function action(task: () => Promise<void>, done: string): Promise<void> {
  busy.value = true
  error.value = ''
  success.value = ''
  try {
    await task()
    await refresh()
    success.value = done
  }
  catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  }
  finally {
    busy.value = false
  }
}

function chooseFile(event: Event): void {
  selectedFile.value = (event.target as HTMLInputElement).files?.[0] ?? null
}

async function install(): Promise<void> {
  if (!selectedFile.value || !trusted.value)
    return
  await action(async () => {
    await getSharedApi().postThemeZip('admin/theme/install', selectedFile.value!)
    selectedFile.value = null
    trusted.value = false
  }, '主题已安装，可在下方启用。')
}

async function activate(short: string): Promise<void> {
  await action(async () => {
    await getSharedApi().post('admin/theme/set', { theme: short })
  }, short === 'emerald' ? '已恢复内置 Emerald 主题。' : '主题已启用，刷新首页即可查看。')
}

async function saveSettings(): Promise<void> {
  const short = listing.value?.active
  if (!short || short === 'emerald')
    return
  let value: unknown
  try {
    value = JSON.parse(settingsDraft.value)
  }
  catch {
    error.value = '主题设置必须是有效的 JSON 对象。'
    return
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    error.value = '主题设置必须是 JSON 对象。'
    return
  }
  await action(async () => {
    await getSharedApi().post(`admin/theme/settings?theme=${encodeURIComponent(short)}`, value)
  }, '主题设置已保存。')
}

onMounted(() => void refresh().catch((cause) => {
  error.value = cause instanceof Error ? cause.message : String(cause)
}))
</script>

<template>
  <section class="space-y-5" aria-labelledby="themes-title">
    <div>
      <h2 id="themes-title" class="text-lg font-semibold">
        主题管理
      </h2>
      <p class="text-sm text-muted-foreground">
        Emerald 是内置默认主题。安装的 Komari 主题在 Pulse 域名下运行；仅安装你信任的主题代码。
      </p>
    </div>

    <p v-if="error" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
      {{ error }}
    </p>
    <p v-if="success" role="status" class="rounded-md border border-success/40 p-3 text-sm text-success">
      {{ success }}
    </p>

    <div class="rounded-xl border bg-card p-4">
      <h3 class="font-medium">
        安装主题包
      </h3>
      <p class="mt-1 text-sm text-muted-foreground">
        上传 ZIP，根目录需包含 komari-theme.json 和 dist/index.html，文件上限 16 MiB。
      </p>
      <div class="mt-4 flex flex-col gap-3">
        <label class="grid gap-2 text-sm">
          主题 ZIP
          <input type="file" accept=".zip,application/zip" class="block min-h-11 rounded-md border px-3 py-2" :disabled="busy" @change="chooseFile">
        </label>
        <label class="flex min-h-11 items-center gap-2 text-sm">
          <input v-model="trusted" type="checkbox" class="size-4 accent-primary" :disabled="busy">
          我信任该主题的发布者及其 JavaScript 代码
        </label>
        <Button class="min-h-11 self-start" :disabled="busy || !selectedFile || !trusted" @click="install">
          {{ busy ? '正在处理…' : '安装主题' }}
        </Button>
      </div>
    </div>

    <div v-if="listing" class="grid gap-3 md:grid-cols-2">
      <article v-for="theme in listing.themes" :key="theme.short" class="rounded-xl border bg-card p-4">
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <h3 class="font-medium">
              {{ localized(theme.name) }}
            </h3>
            <p class="text-sm text-muted-foreground">
              {{ theme.short }} · {{ theme.version }}
            </p>
          </div>
          <span v-if="listing.active === theme.short" class="rounded-full bg-success/15 px-2 py-1 text-xs text-success">当前使用</span>
        </div>
        <p v-if="theme.description" class="mt-2 text-sm text-muted-foreground">
          {{ localized(theme.description) }}
        </p>
        <Button v-if="listing.active !== theme.short" variant="outline" class="mt-4 min-h-11" :disabled="busy" @click="activate(theme.short)">
          启用主题
        </Button>
      </article>
    </div>
    <p v-else class="text-sm text-muted-foreground" role="status">
      正在读取主题列表…
    </p>

    <div v-if="listing && listing.active !== 'emerald'" class="rounded-xl border bg-card p-4">
      <h3 class="font-medium">
        当前主题设置
      </h3>
      <p class="mt-1 text-sm text-muted-foreground">
        根据主题清单中的默认值生成；保存前请确认字段含义。仅接受 JSON 对象，最多 16 KiB。
      </p>
      <label class="mt-3 grid gap-2 text-sm">
        设置 JSON
        <textarea v-model="settingsDraft" class="min-h-40 w-full rounded-md border bg-background p-3 font-mono text-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring" spellcheck="false" :disabled="busy" />
      </label>
      <Button class="mt-3 min-h-11" :disabled="busy" @click="saveSettings">
        保存主题设置
      </Button>
    </div>
  </section>
</template>
