<script setup lang="ts">
import type { EChartsOption } from 'echarts'
import type { ProbeHistory } from '@/utils/admin'
import { Icon } from '@iconify/vue'
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import VChart from 'vue-echarts'
import { Button } from '@/components/ui/button'
import { DataTooltip } from '@/components/ui/data-tooltip'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useBackgroundSurface } from '@/composables/useBackgroundSurface'
import { useAppStore } from '@/stores/app'
import { fetchProbeHistory } from '@/utils/admin'
import { probeChartSeries } from '@/utils/probeChart'
import '@/utils/echarts'

const props = defineProps<{ uuid: string, kind?: 'icmp' | 'tcp' | 'http', taskId?: string, selectedHours?: number }>()
const app = useAppStore()
const { pickSurfaceClass } = useBackgroundSurface()
const hours = ref(props.selectedHours ?? 1)
const showDelay = ref(true)
const showFailures = ref(true)
const cutPeak = ref(false)
const presetRanges = [1, 6, 12, 24, 168, 720, 1440, 2160]
const availableRanges = computed(() => {
  const max = app.publicSettings?.ping_record_preserve_time ?? 168
  const presets = presetRanges.filter(range => range <= max)
  if (max > 1 && !presets.includes(max))
    presets.push(max)
  return presets
})
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
    const response = await fetchProbeHistory(props.uuid, hours.value, props.taskId, controller.signal)
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

watch(() => props.selectedHours, (value) => {
  if (value !== undefined)
    hours.value = value
})
watch(availableRanges, (available) => {
  if (props.selectedHours === undefined && !available.includes(hours.value))
    hours.value = available.at(-1) ?? 1
})
watch(() => [props.uuid, props.taskId, hours.value], () => {
  data.value = null
  void reload()
}, { immediate: true })
onBeforeUnmount(() => {
  clearTimeout(refreshTimer)
  ++generation
  controller?.abort()
})
const tasks = computed(() => (data.value?.tasks ?? []).filter(task => (!props.kind || task.kind === props.kind) && (!props.taskId || task.id === props.taskId)))
const taskIds = computed(() => new Set(tasks.value.map(task => task.id)))
const selectedTaskIds = ref<string[]>([])
let selectionInitialized = false
watch(tasks, (available) => {
  const ids = available.map(task => task.id)
  if (!ids.length)
    return
  if (!selectionInitialized && ids.length) {
    selectedTaskIds.value = ids
    selectionInitialized = true
  }
  else if (selectionInitialized) {
    selectedTaskIds.value = selectedTaskIds.value.filter(id => ids.includes(id))
  }
})
watch(() => [props.uuid, props.taskId, props.kind], () => {
  selectionInitialized = false
  selectedTaskIds.value = []
})
const selectedTasks = computed(() => tasks.value.filter(task => selectedTaskIds.value.includes(task.id)))
const summaryByTask = computed(() => new Map(data.value?.summary.map(summary => [summary.task_id, summary]) ?? []))
const names = computed(() => new Map(data.value?.tasks.map(task => [task.id, task.name]) ?? []))
const recent = computed(() => [...(data.value?.records ?? [])].filter(record => taskIds.value.has(record.task_id)).sort((a, b) => b.received_at_unix_ms - a.received_at_unix_ms).slice(0, 100))
const date = (value: number) => new Date(value).toLocaleString()
const latency = (value: number | null | undefined) => value == null ? '无成功样本' : `${value.toFixed(1)} ms`
const cardLatency = (value: number | null | undefined) => value == null ? '-' : `${Math.round(value)}ms`
function taskStatsLabel(task: ProbeHistory['tasks'][number]): string {
  const stats = summaryByTask.value.get(task.id)
  if (!stats)
    return `${task.name}，暂无上报样本，间隔 ${task.interval_seconds} 秒，类型 ${task.kind.toUpperCase()}`
  return `${task.name}，最低 ${latency(stats.min_latency_ms)}，最高 ${latency(stats.max_latency_ms)}，平均 ${latency(stats.avg_latency_ms)}，最新 ${latency(stats.latest_latency_ms)}，P50 ${latency(stats.p50_latency_ms)}，P95 ${latency(stats.p95_latency_ms)}，P99 ${latency(stats.p99_latency_ms)}，波动率 ${stats.p99_p50_ratio == null ? '不可计算' : stats.p99_p50_ratio.toFixed(2)}，间隔 ${task.interval_seconds} 秒，类型 ${task.kind.toUpperCase()}，样本 ${stats.samples}`
}
const colors = ['#FF6B6B', '#4ECDC4', '#A78BFA', '#60A5FA', '#FFB347', '#F472B6', '#34D399', '#FB923C']
const taskColor = (id: string) => colors[tasks.value.findIndex(task => task.id === id) % colors.length] ?? colors[0]!
const rangeLabel = (range: number) => range < 24 || range % 24 !== 0 ? `${range} 小时` : `${range / 24} 天`
function tooltipTime(value: number): string {
  const date = new Date(value)
  const pad = (number: number) => String(number).padStart(2, '0')
  const clock = `${pad(date.getHours())}:${pad(date.getMinutes())}`
  return hours.value < 24 ? `${clock}:${pad(date.getSeconds())}` : `${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${clock}`
}

function chartTooltip(params: unknown): HTMLElement {
  const root = document.createElement('div')
  const points = (Array.isArray(params) ? params : [params]) as { seriesName?: unknown, data?: unknown, value?: unknown, color?: unknown }[]
  const rows = points.flatMap((point) => {
    if (!point || typeof point !== 'object')
      return []
    const pair = Array.isArray(point.data) ? point.data : point.value
    if (!Array.isArray(pair) || typeof pair[0] !== 'number' || !Number.isFinite(pair[0]) || typeof pair[1] !== 'number' || !Number.isFinite(pair[1]))
      return []
    return [{ time: pair[0] as number, value: pair[1] as number, name: String(point.seriesName ?? ''), color: typeof point.color === 'string' ? point.color : colors[0]! }]
  }).sort((a, b) => a.value - b.value)
  if (!rows.length)
    return root

  const title = document.createElement('div')
  title.style.cssText = 'font-weight:600;margin-bottom:6px'
  title.textContent = tooltipTime(rows[0]!.time)
  root.append(title)
  const list = document.createElement('div')
  list.style.cssText = 'display:flex;flex-direction:column;gap:4px'
  for (const row of rows) {
    const line = document.createElement('div')
    line.style.cssText = 'display:flex;align-items:center'
    const dot = document.createElement('span')
    dot.style.cssText = 'display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:8px;flex-shrink:0'
    dot.style.backgroundColor = row.color
    const name = document.createElement('span')
    name.style.cssText = 'flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap'
    name.textContent = row.name
    const value = document.createElement('span')
    value.style.cssText = 'margin-left:16px;font-weight:600;font-variant-numeric:tabular-nums'
    value.textContent = `${Math.round(row.value)} ms`
    line.append(dot, name, value)
    list.append(line)
  }
  root.append(list)
  return root
}
function toggleTask(id: string): void {
  selectedTaskIds.value = selectedTaskIds.value.includes(id)
    ? selectedTaskIds.value.filter(selected => selected !== id)
    : [...selectedTaskIds.value, id]
}

function taskChartData(id: string): Array<[number, number | null]> {
  const records = (data.value?.chart ?? []).filter(record => record.task_id === id)
  if (!data.value)
    return []
  return probeChartSeries(records, data.value.range_start_unix_ms, data.value.range_end_unix_ms, data.value.chart_bucket_ms, cutPeak.value)
}

const option = computed<EChartsOption>(() => {
  const style = getComputedStyle(document.documentElement)
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
      renderMode: 'html',
      confine: true,
      backgroundColor: app.isDark ? 'rgba(40,40,40,0.95)' : 'rgba(255,255,255,0.8)',
      borderColor: 'transparent',
      borderWidth: 0,
      borderRadius: 6,
      textStyle: { color: foreground, fontSize: 12, lineHeight: 20 },
      axisPointer: { type: 'cross', crossStyle: { color: secondary }, lineStyle: { color: border, width: 1, type: 'dashed' } },
      formatter: chartTooltip,
    },
    legend: { type: 'scroll', bottom: 0, itemWidth: 12, itemHeight: 12, itemGap: 16, icon: 'roundRect', textStyle: { fontSize: 11, color: secondary } },
    grid: { left: 56, right: 24, top: 30, bottom: 52 },
    xAxis: {
      type: 'time',
      min: data.value?.range_start_unix_ms,
      max: data.value?.range_end_unix_ms,
      axisLabel: { fontSize: 11, color: secondary, margin: 12 },
      axisLine: { show: true, lineStyle: { color: border, width: 1 } },
      axisTick: { show: false },
      splitLine: { show: false },
    },
    yAxis: {
      type: 'value',
      name: '延迟 (ms)',
      min: 0,
      nameTextStyle: { color: secondary },
      axisLabel: { fontSize: 11, color: secondary },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: grid, type: 'dashed' } },
    },
    series: selectedTasks.value.map(task => ({
      name: task.name,
      type: 'line',
      showSymbol: false,
      symbolSize: 3,
      smooth: showDelay.value ? (cutPeak.value ? 0.6 : 0.1) : 0,
      lineStyle: { width: showDelay.value ? 1.5 : 0, cap: 'round', color: taskColor(task.id) },
      itemStyle: { color: taskColor(task.id), opacity: showDelay.value ? 1 : 0 },
      connectNulls: false,
      data: taskChartData(task.id),
      markLine: showFailures.value
        ? {
            silent: true,
            symbol: ['none', 'none'],
            animation: false,
            label: { show: false },
            lineStyle: { color: taskColor(task.id), width: 1, opacity: 0.55 },
            data: (data.value?.chart ?? []).filter(record => record.task_id === task.id && record.failed_samples > 0).slice(0, 200).map(record => ({ xAxis: record.received_at_unix_ms })),
          }
        : undefined,
    })),
  }
})
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center gap-2">
      <Tabs v-if="selectedHours === undefined" class="min-w-0 max-w-full flex-1" :model-value="String(hours)" @update:model-value="hours = Number($event)">
        <div class="min-w-0 overflow-x-auto">
          <TabsList :class="pickSurfaceClass('w-max h-8 bg-background/60 rounded-md', 'w-max h-8 bg-background/50 backdrop-blur-xl rounded-md')" aria-label="探测历史时间范围">
            <TabsTrigger
              v-for="range in availableRanges" :key="range" :value="String(range)"
              class="h-6.5 shrink-0 text-xs border-none data-[state=active]:text-emerald-700 dark:data-[state=active]:text-emerald-400 shadow-none rounded-sm"
            >
              {{ rangeLabel(range) }}
            </TabsTrigger>
          </TabsList>
        </div>
      </Tabs>
      <div class="flex items-center gap-2">
        <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="selectedTaskIds.length === tasks.length && 'bg-background !text-emerald-700 dark:!text-emerald-400'" @click="selectedTaskIds = tasks.map(task => task.id)">
          全选
        </Button>
        <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="!selectedTaskIds.length && 'bg-background !text-emerald-700 dark:!text-emerald-400'" @click="selectedTaskIds = []">
          全不选
        </Button>
      </div>
    </div>
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p>
    <p v-if="loading && !data" role="status" class="text-sm text-muted-foreground">
      正在读取探测记录…
    </p>
    <template v-if="data">
      <p v-if="!tasks.length" class="text-sm text-muted-foreground">
        暂无延迟数据
      </p>
      <template v-else>
        <div class="grid gap-3" style="grid-template-columns: repeat(auto-fit, minmax(180px, 1fr))">
          <div
            v-for="task in tasks" :key="task.id"
            class="relative flex min-h-20 items-center rounded-md p-2 transition-all bg-background/60 hover:bg-background hover:shadow-[0_0_0_1px] hover:shadow-emerald-600/10"
            :class="!selectedTaskIds.includes(task.id) && 'ring-1 ring-border'"
          >
            <button
              type="button" class="flex min-h-16 min-w-0 w-full flex-col cursor-pointer text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-emerald-600"
              :aria-pressed="selectedTaskIds.includes(task.id)" :aria-label="`${task.name}，平均延迟 ${latency(summaryByTask.get(task.id)?.avg_latency_ms)}，${task.kind === 'icmp' ? '丢包率' : '失败率'} ${summaryByTask.get(task.id)?.samples ? `${summaryByTask.get(task.id)!.loss_percent.toFixed(2)}%` : '无样本'}，P95 ${latency(summaryByTask.get(task.id)?.p95_latency_ms)}，${selectedTaskIds.includes(task.id) ? '已选中' : '未选中'}`"
              @click="toggleTask(task.id)"
            >
              <span class="flex min-w-0 items-center gap-2 pr-11">
                <span class="h-4 w-1 shrink-0 rounded" :style="{ backgroundColor: taskColor(task.id) }" aria-hidden="true" />
                <span class="truncate text-sm font-semibold" :title="task.name">{{ task.name }}</span>
              </span>
              <span class="mt-2 grid w-full grid-cols-3 gap-2 text-xs">
                <span><span class="block text-muted-foreground">平均</span><span class="font-semibold tabular-nums">{{ cardLatency(summaryByTask.get(task.id)?.avg_latency_ms) }}</span></span>
                <span><span class="block text-muted-foreground">{{ task.kind === 'icmp' ? '丢包' : '失败' }}</span><span class="font-semibold tabular-nums">{{ summaryByTask.get(task.id)?.samples ? `${summaryByTask.get(task.id)!.loss_percent.toFixed(2)}%` : '—' }}</span></span>
                <span><span class="block text-muted-foreground">P95</span><span class="font-semibold tabular-nums">{{ cardLatency(summaryByTask.get(task.id)?.p95_latency_ms) }}</span></span>
              </span>
            </button>
            <DataTooltip placement="left" class="absolute right-1 top-1" content-class="!rounded !bg-popover !text-popover-foreground border border-border p-3 w-60 shadow-lg backdrop-blur max-sm:!top-full max-sm:!right-0 max-sm:!left-auto max-sm:!mt-2 max-sm:!mr-0 max-sm:!translate-y-0">
              <button type="button" class="flex h-11 w-11 cursor-pointer items-center justify-center rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-emerald-600" :aria-label="taskStatsLabel(task)">
                <Icon icon="lucide:info" :width="14" :height="14" aria-hidden="true" />
              </button>
              <template #content>
                <div class="grid grid-cols-4 gap-x-3 gap-y-1.5 text-xs">
                  <template v-for="(metric, index) in [
                    ['最小', summaryByTask.get(task.id)?.min_latency_ms],
                    ['最大', summaryByTask.get(task.id)?.max_latency_ms],
                    ['平均', summaryByTask.get(task.id)?.avg_latency_ms],
                    ['最新', summaryByTask.get(task.id)?.latest_latency_ms],
                    ['P50', summaryByTask.get(task.id)?.p50_latency_ms],
                    ['P95', summaryByTask.get(task.id)?.p95_latency_ms],
                    ['P99', summaryByTask.get(task.id)?.p99_latency_ms],
                  ]" :key="index">
                    <span class="text-muted-foreground">{{ metric[0] }}</span>
                    <span class="font-medium tabular-nums">{{ metric[1] == null ? '—' : `${Math.round(Number(metric[1]))} ms` }}</span>
                  </template>
                  <span class="text-muted-foreground">波动率</span>
                  <span class="font-medium tabular-nums">{{ summaryByTask.get(task.id)?.p99_p50_ratio == null ? '—' : summaryByTask.get(task.id)!.p99_p50_ratio!.toFixed(2) }}</span>
                  <span class="text-muted-foreground">间隔</span>
                  <span class="font-medium tabular-nums">{{ task.interval_seconds }}s</span>
                  <span class="text-muted-foreground">类型</span>
                  <span class="font-medium">{{ task.kind.toUpperCase() }}</span>
                  <span class="text-muted-foreground">总数</span>
                  <span class="font-medium tabular-nums">{{ summaryByTask.get(task.id)?.samples ?? 0 }}</span>
                </div>
              </template>
            </DataTooltip>
          </div>
        </div>
        <div class="flex flex-wrap items-center gap-2 py-2">
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="showDelay && 'bg-background !text-emerald-700 dark:!text-emerald-400'" :aria-pressed="showDelay" @click="showDelay = !showDelay">
            延迟
          </Button>
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="showFailures && 'bg-background !text-emerald-700 dark:!text-emerald-400'" :aria-pressed="showFailures" @click="showFailures = !showFailures">
            失败标记
          </Button>
          <div class="flex items-center gap-1">
            <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="cutPeak && 'bg-background !text-emerald-700 dark:!text-emerald-400'" :aria-pressed="cutPeak" @click="cutPeak = !cutPeak">
              平滑峰值
            </Button>
            <DataTooltip content="使用 EWMA 算法平滑曲线并过滤突变值；汇总仍按原始样本计算" placement="top" content-class="max-w-56 text-[11px] leading-relaxed">
              <button type="button" class="flex h-7 w-7 items-center justify-center rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-emerald-600" aria-label="平滑峰值说明">
                <Icon icon="lucide:info" :width="14" :height="14" aria-hidden="true" />
              </button>
            </DataTooltip>
          </div>
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :disabled="loading" @click="reload">
            {{ loading ? '加载中…' : '刷新' }}
          </Button>
        </div>
        <p v-if="data.chart_source_points > data.chart.length" class="text-sm text-muted-foreground">
          曲线覆盖整个所选时间范围，已将 {{ data.chart_source_points }} 个样本按时间聚合为 {{ data.chart.length }} 个点；汇总和最近记录仍使用原始样本。
        </p>
        <div v-if="data.chart.length" class="h-80 rounded-md p-4" :class="pickSurfaceClass('bg-background/60', 'bg-background/50 backdrop-blur-xl')">
          <VChart :key="app.resolvedThemeMode" :option="option" autoresize style="height: 100%; width: 100%" aria-label="探测延迟曲线，长时间缺报为断点，可显示失败标记" />
        </div>
        <p v-else class="text-sm text-muted-foreground">
          此时间范围暂无上报样本。
        </p>
        <p class="text-xs text-muted-foreground">
          失败标记表示探测无响应；TCP、HTTP 失败不等同于 ICMP 丢包。
        </p>
        <details class="relative rounded-md border p-3 bg-background/50">
          <summary class="cursor-pointer text-sm font-medium">
            延迟与失败率汇总
          </summary>
          <div class="overflow-x-auto">
            <table class="w-full text-left text-sm">
              <caption class="sr-only">
                所选时间范围的延迟与失败率汇总
              </caption><thead>
                <tr class="border-b">
                  <th class="p-2">
                    任务
                  </th><th class="p-2">
                    样本
                  </th><th class="p-2">
                    平均延迟
                  </th><th class="p-2">
                    最低 / 最高
                  </th><th class="p-2">
                    最新
                  </th><th class="p-2">
                    P50 / P95 / P99
                  </th><th class="p-2">
                    波动率
                  </th><th class="p-2">
                    {{ props.kind === 'icmp' ? '丢包率' : '失败率' }}
                  </th>
                </tr>
              </thead><tbody>
                <tr v-for="task in tasks" :key="task.id" class="border-b">
                  <th class="p-2 font-medium">
                    {{ task.name }}
                  </th><td class="p-2">
                    {{ summaryByTask.get(task.id)?.samples ?? 0 }}
                  </td><td class="p-2">
                    {{ latency(summaryByTask.get(task.id)?.avg_latency_ms) }}
                  </td><td class="whitespace-nowrap p-2">
                    {{ latency(summaryByTask.get(task.id)?.min_latency_ms) }} / {{ latency(summaryByTask.get(task.id)?.max_latency_ms) }}
                  </td><td class="p-2">
                    {{ latency(summaryByTask.get(task.id)?.latest_latency_ms) }}
                  </td><td class="whitespace-nowrap p-2">
                    {{ latency(summaryByTask.get(task.id)?.p50_latency_ms) }} / {{ latency(summaryByTask.get(task.id)?.p95_latency_ms) }} / {{ latency(summaryByTask.get(task.id)?.p99_latency_ms) }}
                  </td><td class="p-2">
                    {{ summaryByTask.get(task.id)?.p99_p50_ratio == null ? '—' : summaryByTask.get(task.id)!.p99_p50_ratio!.toFixed(2) }}
                  </td><td class="p-2">
                    {{ summaryByTask.get(task.id)?.samples ? `${summaryByTask.get(task.id)!.loss_percent.toFixed(1)}%` : '无样本' }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </details>
        <details v-if="recent.length" class="relative rounded-md border p-3 bg-background/50">
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
  </div>
</template>
