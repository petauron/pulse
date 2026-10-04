<script setup lang="ts">
import type { EChartsOption } from 'echarts'
import type { ProbeHistory } from '@/utils/admin'
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import VChart from 'vue-echarts'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAppStore } from '@/stores/app'
import { fetchProbeHistory } from '@/utils/admin'
import '@/utils/echarts'

const props = defineProps<{ uuid: string }>()
const app = useAppStore()
const hours = ref(1)
const ranges = [1, 6, 12, 24, 72, 168]
let refreshTimer: ReturnType<typeof setTimeout> | undefined
const data = shallowRef<ProbeHistory | null>(null)
const loading = ref(false)
const error = ref('')
let controller: AbortController | null = null
let generation = 0

async function reload(): Promise<void> {
  clearTimeout(refreshTimer)
  controller?.abort()
  const current = ++generation
  controller = new AbortController()
  loading.value = true
  error.value = ''
  try {
    const response = await fetchProbeHistory(props.uuid, hours.value, controller.signal)
    if (current === generation)
      data.value = response
  }
  catch (cause) {
    if (current === generation)
      error.value = cause instanceof Error ? cause.message : String(cause)
  }
  finally {
    if (current === generation) {
      loading.value = false
      refreshTimer = setTimeout(() => void reload(), 30000)
    }
  }
}

watch(() => [props.uuid, hours.value], () => {
  data.value = null
  void reload()
}, { immediate: true })
onBeforeUnmount(() => {
  clearTimeout(refreshTimer)
  ++generation
  controller?.abort()
})
const names = computed(() => new Map(data.value?.tasks.map(task => [task.id, task.name]) ?? []))
const recent = computed(() => [...(data.value?.records ?? [])].sort((a, b) => b.received_at_unix_ms - a.received_at_unix_ms).slice(0, 100))
const date = (value: number) => new Date(value).toLocaleString()
const latency = (value: number | null | undefined) => value == null ? '无成功样本' : `${value.toFixed(1)} ms`

const option = computed<EChartsOption>(() => {
  const style = getComputedStyle(document.documentElement)
  const colors = ['#FF6B6B', '#4ECDC4', '#A78BFA', '#60A5FA', '#FFB347', '#F472B6', '#34D399', '#FB923C']
  const foreground = style.getPropertyValue('--foreground').trim()
  const secondary = app.isDark ? 'rgba(255,255,255,0.55)' : 'rgba(0,0,0,0.55)'
  const border = app.isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.06)'
  const grid = app.isDark ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.06)'
  return {
    darkMode: app.isDark,
    animation: false,
    color: colors,
    aria: { enabled: true },
    textStyle: { color: foreground },
    tooltip: {
      trigger: 'axis',
      renderMode: 'richText',
      confine: true,
      backgroundColor: app.isDark ? 'rgba(40,40,40,0.95)' : 'rgba(255,255,255,0.8)',
      borderColor: 'transparent',
      borderWidth: 0,
      borderRadius: 6,
      textStyle: { color: foreground, fontSize: 12, lineHeight: 20 },
      axisPointer: { type: 'cross', crossStyle: { color: secondary }, lineStyle: { color: border, width: 1, type: 'dashed' } },
    },
    legend: { type: 'scroll', bottom: 0, itemWidth: 12, itemHeight: 8, itemGap: 14, textStyle: { fontSize: 10, color: secondary } },
    grid: { left: 56, right: 56, top: 30, bottom: 52 },
    xAxis: {
      type: 'time',
      axisLabel: { fontSize: 11, color: secondary, margin: 12 },
      axisLine: { show: true, lineStyle: { color: border, width: 1 } },
      axisTick: { show: false },
      splitLine: { show: false },
    },
    yAxis: {
      type: 'value', name: '延迟 (ms)', min: 0,
      nameTextStyle: { color: secondary },
      axisLabel: { fontSize: 11, color: secondary },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: grid, type: 'dashed' } },
    },
    series: (data.value?.tasks ?? []).map(task => ({
      name: task.name,
      type: 'line',
      showSymbol: (data.value?.records ?? []).filter(record => record.task_id === task.id && record.success && record.latency_ms !== null).length === 1,
      symbolSize: 3,
      smooth: 0.1,
      lineStyle: { width: 1.8, cap: 'round' },
      connectNulls: false,
      data: (data.value?.records ?? []).filter(record => record.task_id === task.id).sort((a, b) => a.received_at_unix_ms - b.received_at_unix_ms).map(record => [record.received_at_unix_ms, record.success ? record.latency_ms : null]),
    })),
  }
})
</script>

<template>
  <CardX title="网络延迟" content-class="space-y-4" header-class="flex-wrap">
    <template #header-extra>
      <div class="flex flex-wrap gap-2">
        <Tabs :model-value="String(hours)" @update:model-value="hours = Number($event)">
          <TabsList class="flex-wrap h-auto" aria-label="探测历史时间范围">
            <TabsTrigger v-for="range in ranges" :key="range" :value="String(range)">
              {{ range < 24 ? `${range} 小时` : `${range / 24} 天` }}
            </TabsTrigger>
          </TabsList>
        </Tabs>
        <Button variant="outline" :disabled="loading" @click="reload">
          {{ loading ? '加载中…' : '刷新' }}
        </Button>
      </div>
    </template>
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p>
    <p v-if="loading && !data" role="status" class="text-sm text-muted-foreground">
      正在读取探测记录…
    </p>
    <template v-if="data">
      <p v-if="!data.tasks.length" class="text-sm text-muted-foreground">
        此节点尚未配置探测任务。管理员可添加 ICMP、TCP 或 HTTP 探测。
      </p>
      <template v-else>
        <p v-if="data.records.length >= data.limit" class="text-sm text-muted-foreground">
          曲线仅展示最近 {{ data.limit }} 个样本；汇总按整个所选时间范围计算。
        </p>
        <div v-if="data.records.length" class="w-full" style="height: 280px">
          <VChart :key="app.resolvedThemeMode" :option="option" autoresize style="height: 100%; width: 100%" aria-label="探测延迟曲线，失败样本显示为断点，数值可在下方表格查看" />
        </div>
        <p v-else class="text-sm text-muted-foreground">
          此时间范围暂无上报样本。
        </p>
        <details class="rounded-md border p-3">
          <summary class="cursor-pointer text-sm font-medium">延迟与失败率汇总</summary>
          <div class="overflow-x-auto">
            <table class="w-full text-left text-sm">
              <caption class="sr-only">
                所选时间范围的延迟与丢包汇总
              </caption><thead>
                <tr class="border-b">
                  <th class="p-2">
                    任务
                  </th><th class="p-2">
                    样本
                  </th><th class="p-2">
                    平均延迟
                  </th><th class="p-2">
                    失败 / 丢包率
                  </th>
                </tr>
              </thead><tbody>
                <tr v-for="task in data.tasks" :key="task.id" class="border-b">
                  <th class="p-2 font-medium">
                    {{ task.name }}
                  </th><td class="p-2">
                    {{ data.summary.find(s => s.task_id === task.id)?.samples ?? 0 }}
                  </td><td class="p-2">
                    {{ latency(data.summary.find(s => s.task_id === task.id)?.avg_latency_ms) }}
                  </td><td class="p-2">
                    {{ data.summary.find(s => s.task_id === task.id)?.samples ? `${data.summary.find(s => s.task_id === task.id)!.loss_percent.toFixed(1)}%` : '无样本' }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </details>
        <details v-if="recent.length" class="rounded-md border p-3">
          <summary class="cursor-pointer text-sm font-medium">
            查看最近 {{ recent.length }} 条记录
          </summary>
          <div class="mt-3 max-h-96 overflow-auto">
            <table class="w-full text-left text-sm">
              <caption class="sr-only">
                探测结果明细（按服务器接收时间倒序）
              </caption><thead>
                <tr>
                  <th class="p-2">
                    接收时间
                  </th><th class="p-2">
                    任务
                  </th><th class="p-2">
                    结果
                  </th>
                </tr>
              </thead><tbody>
                <tr v-for="(record, index) in recent" :key="index" class="border-t">
                  <td class="whitespace-nowrap p-2">
                    {{ date(record.received_at_unix_ms) }}
                  </td><td class="p-2">
                    {{ names.get(record.task_id) ?? record.task_id }}
                  </td><td class="p-2" :class="record.success ? '' : 'text-destructive'">
                    {{ record.success ? latency(record.latency_ms) : `失败：${record.error || '无响应'}` }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </details>
      </template>
    </template>
  </CardX>
</template>
