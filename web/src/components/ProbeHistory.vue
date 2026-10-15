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
import { probeChartSeries, probeFailureSeries } from '@/utils/probeChart'
import '@/utils/echarts'

const props = defineProps<{ uuid: string, kind?: 'icmp' | 'tcp' | 'http', taskId?: string, selectedHours?: number }>()
const app = useAppStore()
const { pickSurfaceClass } = useBackgroundSurface()
const hours = ref(props.selectedHours ?? 1)
const showDelay = ref(true)
const showFailures = ref(true)
const cutPeak = ref(false)
const chartLayout = ref<'combined' | 'split'>('combined')
const autoRefresh = ref(true)
const combinedChartRef = ref<unknown>(null)
const latencyChartRef = ref<unknown>(null)
const failureChartRef = ref<unknown>(null)
const exportError = ref('')
const filenameUnsafeChars = /[^\w-]/g
const spreadsheetFormulaPrefix = /^\s*[-=+@]/
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
  refreshTimer = undefined
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
      if (autoRefresh.value)
        refreshTimer = setTimeout(() => void reload(), 30000)
    }
  }
}

function toggleAutoRefresh(): void {
  autoRefresh.value = !autoRefresh.value
  if (autoRefresh.value) {
    void reload()
  }
  else {
    clearTimeout(refreshTimer)
    refreshTimer = undefined
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
const analysisTaskId = ref('')
let selectionInitialized = false
watch(tasks, (available) => {
  const ids = available.map(task => task.id)
  if (!ids.includes(analysisTaskId.value))
    analysisTaskId.value = ids[0] ?? ''
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
  analysisTaskId.value = ''
})
const selectedTasks = computed(() => tasks.value.filter(task => selectedTaskIds.value.includes(task.id)))
const summaryByTask = computed(() => new Map(data.value?.summary.map(summary => [summary.task_id, summary]) ?? []))
const analysisTask = computed(() => tasks.value.find(task => task.id === analysisTaskId.value))
const analysisSummary = computed(() => summaryByTask.value.get(analysisTaskId.value))
const names = computed(() => new Map(data.value?.tasks.map(task => [task.id, task.name]) ?? []))
const recent = computed(() => [...(data.value?.records ?? [])].filter(record => taskIds.value.has(record.task_id)).sort((a, b) => b.received_at_unix_ms - a.received_at_unix_ms).slice(0, 100))
const date = (value: number) => new Date(value).toLocaleString()
const latency = (value: number | null | undefined) => value == null ? '无成功样本' : `${value.toFixed(1)} ms`
const cardLatency = (value: number | null | undefined) => value == null ? '-' : `${Math.round(value)}ms`
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
  const points = (Array.isArray(params) ? params : [params]) as { seriesId?: unknown, seriesName?: unknown, data?: unknown, value?: unknown, color?: unknown }[]
  const rows = points.flatMap((point) => {
    if (!point || typeof point !== 'object')
      return []
    const pair = Array.isArray(point.data) ? point.data : point.value
    if (!Array.isArray(pair) || typeof pair[0] !== 'number' || !Number.isFinite(pair[0]) || typeof pair[1] !== 'number' || !Number.isFinite(pair[1]))
      return []
    return [{ time: pair[0] as number, value: pair[1] as number, name: String(point.seriesName ?? ''), failureRate: typeof point.seriesId === 'string' && point.seriesId.endsWith(':failure-rate'), color: typeof point.color === 'string' ? point.color : colors[0]! }]
  }).sort((a, b) => Number(a.failureRate) - Number(b.failureRate) || a.value - b.value)
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
    value.textContent = row.failureRate ? `${row.value.toFixed(1)}%` : `${Math.round(row.value)} ms`
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

function taskFailureData(id: string): Array<[number, number | null]> {
  const records = (data.value?.chart ?? []).filter(record => record.task_id === id)
  if (!data.value)
    return []
  return probeFailureSeries(records, data.value.range_start_unix_ms, data.value.range_end_unix_ms, data.value.chart_bucket_ms)
}

const failureLabel = (kind: ProbeHistory['tasks'][number]['kind']) => kind === 'icmp' ? '丢包率' : '失败率'
const failureMetricLabel = computed(() => tasks.value.every(task => task.kind === 'icmp') ? '丢包率' : tasks.value.every(task => task.kind !== 'icmp') ? '失败率' : '丢包 / 失败率')
const showZoom = computed(() => app.publicSettings?.theme_settings?.pingChartShowZoom !== false)
const chartThresholds = computed(() => {
  const settings = app.publicSettings?.theme_settings
  const value = (key: string, fallback: number, min: number, max: number) => {
    const candidate = settings?.[key]
    return typeof candidate === 'number' && Number.isFinite(candidate) && candidate >= min && candidate <= max ? candidate : fallback
  }
  return {
    latencyWarning: value('pingLatencyWarning', 160, 1, 5000),
    latencyCritical: value('pingLatencyCritical', 300, 1, 5000),
    failureWarning: value('pingLossWarning', 3, 0, 100),
    failureCritical: value('pingLossCritical', 10, 0, 100),
  }
})

function chartOption(mode: 'combined' | 'latency' | 'failure'): EChartsOption {
  const style = getComputedStyle(document.documentElement)
  const foreground = style.getPropertyValue('--foreground').trim()
  const secondary = app.isDark ? 'rgba(255,255,255,0.55)' : 'rgba(0,0,0,0.55)'
  const border = app.isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.06)'
  const grid = app.isDark ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.06)'
  const latencySeries = showDelay.value && mode !== 'failure'
    ? selectedTasks.value.map((task, index) => ({
        id: `${task.id}:latency`,
        name: task.name,
        type: 'line' as const,
        yAxisIndex: 0,
        showSymbol: false,
        symbolSize: 3,
        smooth: cutPeak.value ? 0.6 : 0.1,
        lineStyle: { width: 1.8, cap: 'round' as const, color: taskColor(task.id) },
        itemStyle: { color: taskColor(task.id) },
        connectNulls: false,
        data: taskChartData(task.id),
        markLine: index === 0 ? {
          silent: true,
          symbol: ['none', 'none'] as [string, string],
          label: { show: true, position: 'insideEndTop' as const, fontSize: 10 },
          lineStyle: { width: 1, type: 'dashed' as const, opacity: 0.65 },
          data: [
            { name: '延迟警告', yAxis: chartThresholds.value.latencyWarning, lineStyle: { color: '#FACC15' } },
            { name: '延迟严重', yAxis: chartThresholds.value.latencyCritical, lineStyle: { color: '#F43F5E' } },
          ],
        } : undefined,
        markPoint: {
          symbolSize: 28,
          label: { fontSize: 9, formatter: '峰值' },
          data: [{ type: 'max' as const, name: '峰值' }],
        },
      }))
    : []
  const failureSeries = showFailures.value && mode !== 'latency'
    ? selectedTasks.value.map((task, index) => {
        const points = taskFailureData(task.id)
        return {
          id: `${task.id}:failure-rate`,
          name: `${task.name} · ${failureLabel(task.kind)}`,
          type: 'line' as const,
          yAxisIndex: mode === 'combined' ? 1 : 0,
          showSymbol: points.length === 1,
          symbolSize: 4,
          step: 'end' as const,
          lineStyle: { width: 1.4, type: 'dashed' as const, color: taskColor(task.id), opacity: 0.8 },
          areaStyle: { color: taskColor(task.id), opacity: 0.05 },
          itemStyle: { color: taskColor(task.id) },
          connectNulls: false,
          data: points,
          markLine: index === 0 ? {
            silent: true,
            symbol: ['none', 'none'] as [string, string],
            label: { show: true, position: 'insideEndTop' as const, fontSize: 10 },
            lineStyle: { width: 1, type: 'dotted' as const, opacity: 0.65 },
            data: [
              { name: `${failureMetricLabel.value}警告`, yAxis: chartThresholds.value.failureWarning, lineStyle: { color: '#FACC15' } },
              { name: `${failureMetricLabel.value}严重`, yAxis: chartThresholds.value.failureCritical, lineStyle: { color: '#F43F5E' } },
            ],
          } : undefined,
        }
      })
    : []
  const latencyAxis = {
    type: 'value' as const,
    name: '延迟 (ms)',
    min: 0,
    nameTextStyle: { color: secondary },
    axisLabel: { fontSize: 11, color: secondary },
    axisLine: { show: false },
    axisTick: { show: false },
    splitLine: { lineStyle: { color: grid, type: 'dashed' as const } },
  }
  const failureAxis = {
    type: 'value' as const,
    name: `${failureMetricLabel.value} (%)`,
    min: 0,
    max: 100,
    position: mode === 'combined' ? 'right' as const : 'left' as const,
    nameTextStyle: { color: secondary },
    axisLabel: { fontSize: 11, color: secondary, formatter: '{value}%' },
    axisLine: { show: false },
    axisTick: { show: false },
    splitLine: { show: mode === 'failure', lineStyle: { color: grid, type: 'dashed' as const } },
  }
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
    legend: { type: 'scroll', bottom: showZoom.value ? 24 : 0, itemWidth: 12, itemHeight: 8, itemGap: 14, icon: 'roundRect', textStyle: { fontSize: 10, color: secondary } },
    dataZoom: showZoom.value ? [
      { type: 'inside', start: 0, end: 100 },
      { type: 'slider', height: 14, bottom: 0, borderColor: 'transparent', fillerColor: app.isDark ? 'rgba(52,211,153,.16)' : 'rgba(5,150,105,.12)', backgroundColor: grid, showDetail: false },
    ] : [{ type: 'inside', start: 0, end: 100 }],
    grid: { left: 56, right: mode === 'combined' ? 66 : 24, top: 30, bottom: showZoom.value ? 72 : 52 },
    xAxis: {
      type: 'time',
      min: data.value?.range_start_unix_ms,
      max: data.value?.range_end_unix_ms,
      axisLabel: { fontSize: 11, color: secondary, margin: 12 },
      axisLine: { show: true, lineStyle: { color: border, width: 1 } },
      axisTick: { show: false },
      splitLine: { show: false },
    },
    yAxis: mode === 'combined' ? [latencyAxis, failureAxis] : [mode === 'latency' ? latencyAxis : failureAxis],
    series: [...latencySeries, ...failureSeries],
  }
}

const combinedOption = computed<EChartsOption>(() => chartOption('combined'))
const latencyOption = computed<EChartsOption>(() => chartOption('latency'))
const failureOption = computed<EChartsOption>(() => chartOption('failure'))

function download(filename: string, href: string): void {
  const link = document.createElement('a')
  link.href = href
  link.download = filename
  document.body.append(link)
  link.click()
  link.remove()
}

const exportFilename = (extension: string) => `senulun-ping-${props.uuid.replace(filenameUnsafeChars, '_')}-${hours.value}h.${extension}`

function exportCsv(): void {
  if (!data.value || !selectedTasks.value.length)
    return
  const selected = selectedTasks.value
  const headers = ['时间', ...selected.flatMap(task => [`${task.name} 延迟(ms)`, `${task.name} ${failureLabel(task.kind)}(%)`])]
  const series = selected.map(task => ({
    latency: new Map(taskChartData(task.id)),
    failure: new Map(taskFailureData(task.id)),
  }))
  const times = [...new Set(data.value.chart.filter(point => selectedTaskIds.value.includes(point.task_id)).map(point => point.received_at_unix_ms))].sort((a, b) => a - b)
  const escapeCsv = (value: string | number | null | undefined) => {
    const text = value == null ? '' : String(value)
    // Spreadsheet applications can execute formula-like fields, including task names in headers.
    const safe = spreadsheetFormulaPrefix.test(text) ? `'${text}` : text
    return `"${safe.replaceAll('"', '""')}"`
  }
  const rows = times.map(time => [new Date(time).toISOString(), ...series.flatMap(task => [task.latency.get(time) ?? '', task.failure.get(time) ?? ''])])
  const csv = `\uFEFF${[headers, ...rows].map(row => row.map(escapeCsv).join(',')).join('\r\n')}`
  const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }))
  download(exportFilename('csv'), url)
  URL.revokeObjectURL(url)
  exportError.value = ''
}

function exportPng(): void {
  const target = chartLayout.value === 'combined'
    ? combinedChartRef.value
    : showDelay.value ? latencyChartRef.value : failureChartRef.value
  const chart = target as { getDataURL?: (options: { type: 'png', pixelRatio: number, backgroundColor: string }) => string } | null
  const url = chart?.getDataURL?.({ type: 'png', pixelRatio: 2, backgroundColor: app.isDark ? '#111827' : '#ffffff' })
  if (!url) {
    exportError.value = '图表尚未准备好，请稍后再试。'
    return
  }
  download(exportFilename('png'), url)
  exportError.value = ''
}
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
        <section aria-label="延迟数据分析" class="flex flex-col gap-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <h3 class="text-sm font-semibold">
              延迟数据分析
            </h3>
            <label class="flex items-center gap-2 text-xs text-muted-foreground">
              分析任务
              <select v-model="analysisTaskId" aria-label="分析任务" class="h-8 max-w-44 cursor-pointer rounded-md border border-border bg-background/60 px-2 text-sm text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-emerald-600 sm:text-xs">
                <option v-for="task in tasks" :key="task.id" :value="task.id">
                  {{ task.name }}
                </option>
              </select>
            </label>
          </div>
          <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
            <div
              v-for="task in tasks" :key="task.id"
              class="min-w-0 rounded-md border bg-background/60 p-3 transition-colors"
              :class="analysisTaskId === task.id ? 'border-emerald-600/60' : 'border-border'"
            >
              <div class="flex min-w-0 items-center gap-2">
                <span class="h-5 w-1 shrink-0 rounded" :style="{ backgroundColor: taskColor(task.id) }" aria-hidden="true" />
                <span class="min-w-0 flex-1 truncate text-sm font-semibold" :title="task.name">{{ task.name }}</span>
                <button type="button" class="flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-emerald-600" :aria-label="`${selectedTaskIds.includes(task.id) ? '隐藏' : '显示'} ${task.name} 曲线`" :aria-pressed="selectedTaskIds.includes(task.id)" @click="toggleTask(task.id)">
                  <Icon :icon="selectedTaskIds.includes(task.id) ? 'lucide:eye' : 'lucide:eye-off'" :width="15" :height="15" aria-hidden="true" />
                </button>
                <button type="button" class="flex h-11 shrink-0 cursor-pointer items-center rounded-sm px-1 text-xs font-medium hover:text-emerald-600 focus-visible:outline-2 focus-visible:outline-emerald-600" :aria-label="`分析 ${task.name}`" :aria-pressed="analysisTaskId === task.id" @click="analysisTaskId = task.id">
                  分析
                </button>
              </div>
              <div class="mt-2 grid grid-cols-3 gap-2 text-xs">
                <span><span class="block text-muted-foreground">平均</span><span class="font-semibold tabular-nums">{{ cardLatency(summaryByTask.get(task.id)?.avg_latency_ms) }}</span></span>
                <span><span class="block text-muted-foreground">{{ task.kind === 'icmp' ? '丢包' : '失败' }}</span><span class="font-semibold tabular-nums">{{ summaryByTask.get(task.id)?.samples ? `${summaryByTask.get(task.id)!.loss_percent.toFixed(2)}%` : '—' }}</span></span>
                <span><span class="block text-muted-foreground">P95</span><span class="font-semibold tabular-nums">{{ cardLatency(summaryByTask.get(task.id)?.p95_latency_ms) }}</span></span>
              </div>
            </div>
          </div>
          <div v-if="analysisTask" class="rounded-md border border-border bg-background/60 p-3">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <h4 class="flex min-w-0 items-center gap-2 text-sm font-semibold">
                <span class="h-5 w-1 shrink-0 rounded" :style="{ backgroundColor: taskColor(analysisTask.id) }" aria-hidden="true" />
                <span class="min-w-0 break-words">{{ analysisTask.name }}</span>
              </h4>
              <Button variant="ghost" size="xs" class="h-8 rounded-sm" @click="selectedTaskIds = [analysisTask.id]">
                仅看此任务
              </Button>
            </div>
            <dl class="mt-4 grid grid-cols-2 gap-x-3 gap-y-4 text-xs sm:grid-cols-3 lg:grid-cols-6">
              <div v-for="metric in [
                { label: '最小延迟', value: latency(analysisSummary?.min_latency_ms) },
                { label: '平均延迟', value: latency(analysisSummary?.avg_latency_ms) },
                { label: '最大延迟', value: latency(analysisSummary?.max_latency_ms) },
                { label: '最新延迟', value: latency(analysisSummary?.latest_latency_ms) },
                { label: 'P50', value: latency(analysisSummary?.p50_latency_ms) },
                { label: 'P95', value: latency(analysisSummary?.p95_latency_ms) },
                { label: 'P99', value: latency(analysisSummary?.p99_latency_ms) },
                { label: analysisTask.kind === 'icmp' ? '丢包率' : '失败率', value: analysisSummary?.samples ? `${analysisSummary.loss_percent.toFixed(2)}%` : '无样本' },
                { label: '波动率', value: analysisSummary?.p99_p50_ratio == null ? '—' : analysisSummary.p99_p50_ratio.toFixed(2) },
                { label: '样本数', value: String(analysisSummary?.samples ?? 0) },
                { label: '采集间隔', value: `${analysisTask.interval_seconds}s` },
                { label: '协议', value: analysisTask.kind.toUpperCase() },
              ]" :key="metric.label">
                <dt class="text-muted-foreground">{{ metric.label }}</dt>
                <dd class="mt-1 font-semibold tabular-nums">{{ metric.value }}</dd>
              </div>
            </dl>
          </div>
        </section>
        <div class="flex flex-wrap items-center gap-2 py-2">
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="showDelay && 'bg-background !text-emerald-700 dark:!text-emerald-400'" :aria-pressed="showDelay" @click="showDelay = !showDelay">
            延迟
          </Button>
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="showFailures && 'bg-background !text-emerald-700 dark:!text-emerald-400'" :aria-pressed="showFailures" @click="showFailures = !showFailures">
            {{ failureMetricLabel }}
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
          <div role="group" aria-label="图表布局" class="flex items-center gap-1">
            <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="chartLayout === 'combined' && 'bg-background !text-emerald-700 dark:!text-emerald-400'" :aria-pressed="chartLayout === 'combined'" @click="chartLayout = 'combined'">
              合图
            </Button>
            <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :class="chartLayout === 'split' && 'bg-background !text-emerald-700 dark:!text-emerald-400'" :aria-pressed="chartLayout === 'split'" @click="chartLayout = 'split'">
              分图
            </Button>
          </div>
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" @click="toggleAutoRefresh">
            {{ autoRefresh ? '暂停刷新' : '自动刷新' }}
          </Button>
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :disabled="loading" @click="reload">
            {{ loading ? '加载中…' : '立即刷新' }}
          </Button>
          <div class="min-w-0 flex-1" />
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :disabled="!data.chart.length || !selectedTasks.length || (!showDelay && !showFailures)" @click="exportPng">
            PNG
          </Button>
          <Button variant="ghost" size="xs" class="h-7 rounded-sm border-none bg-background/60 hover:bg-background" :disabled="!data.chart.length || !selectedTasks.length" @click="exportCsv">
            CSV
          </Button>
        </div>
        <p v-if="exportError" role="alert" class="text-sm text-destructive">
          {{ exportError }}
        </p>
        <p v-if="data.chart_source_points > data.chart.length" class="text-sm text-muted-foreground">
          曲线覆盖整个所选时间范围，已将 {{ data.chart_source_points }} 个样本按时间聚合为 {{ data.chart.length }} 个点；汇总和最近记录仍使用原始样本。
        </p>
        <template v-if="data.chart.length && (showDelay || showFailures)">
          <div v-if="chartLayout === 'combined'" class="h-88 rounded-md p-3 md:h-104" :class="pickSurfaceClass('bg-background/60', 'bg-background/50 backdrop-blur-xl')">
            <VChart :key="app.resolvedThemeMode" ref="combinedChartRef" :option="combinedOption" autoresize style="height: 100%; width: 100%" aria-label="探测延迟和丢包或失败率曲线，左右轴分别为毫秒和百分比，长时间缺报为断点" />
          </div>
          <template v-else>
            <div v-if="showDelay" class="h-72 rounded-md p-3 md:h-80" :class="pickSurfaceClass('bg-background/60', 'bg-background/50 backdrop-blur-xl')">
              <VChart :key="app.resolvedThemeMode" ref="latencyChartRef" :option="latencyOption" autoresize style="height: 100%; width: 100%" aria-label="独立延迟曲线，纵轴为毫秒，长时间缺报为断点" />
            </div>
            <div v-if="showFailures" class="h-72 rounded-md p-3 md:h-80" :class="pickSurfaceClass('bg-background/60', 'bg-background/50 backdrop-blur-xl')">
              <VChart :key="app.resolvedThemeMode" ref="failureChartRef" :option="failureOption" autoresize style="height: 100%; width: 100%" aria-label="独立丢包或失败率曲线，纵轴为百分比，长时间缺报为断点" />
            </div>
          </template>
        </template>
        <p v-else-if="!data.chart.length" class="text-sm text-muted-foreground">
          此时间范围暂无上报样本。
        </p>
        <p v-else class="text-sm text-muted-foreground">
          请选择要显示的曲线。
        </p>
        <p class="text-xs text-muted-foreground">
          百分比轴是每个有样本时间桶的失败比例；较长缺报会断开，不计为 0%。TCP、HTTP 失败不等同于 ICMP 丢包。
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
