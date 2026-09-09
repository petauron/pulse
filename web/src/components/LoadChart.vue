<script setup lang="ts">
import type { RecordFormat } from '@/utils/recordHelper'
import type { HistoryCoverage, StatusRecord } from '@/utils/rpc'
import { Icon } from '@iconify/vue'
import { useIntervalFn } from '@vueuse/core'
import dayjs from 'dayjs'
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import VChart from 'vue-echarts'
import { CardX } from '@/components/ui/card-x'
import { Empty } from '@/components/ui/empty'
import { Spinner } from '@/components/ui/spinner'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useBackgroundSurface } from '@/composables/useBackgroundSurface'
import { useAppStore } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { formatBytes, formatBytesSplit } from '@/utils/helper'
import { alignRecordsToTimeWindow } from '@/utils/recordHelper'
import { getSharedRpc } from '@/utils/rpc'
import '@/utils/echarts' // 共享 ECharts 配置

const props = defineProps<{
  uuid: string
}>()

const appStore = useAppStore()
const { pickSurfaceClass } = useBackgroundSurface()
const nodesStore = useNodesStore()

// 从 publicSettings 获取记录保留时间
const maxRecordPreserveTime = computed(() => appStore.publicSettings?.record_preserve_time || 720)

// 从 publicSettings.theme_settings 获取数据更新间隔（秒），默认 3 秒
const dataUpdateInterval = computed(() => {
  const settings = appStore.publicSettings?.theme_settings
  const interval = settings?.dataUpdateInterval
  // 确保值在合理范围内（1-60秒）
  if (typeof interval === 'number' && interval >= 1 && interval <= 60) {
    return interval * 1000 // 转换为毫秒
  }
  return 3000 // 默认 3 秒
})

// 使用 store 中的 isDark computed
const isDark = computed(() => appStore.isDark)

// 优化后的图表配色方案（基于 Material Design 色彩）
const chartColors = {
  // 主色调 - 珊瑚红
  primary: '#FF6B6B',
  primaryArea: 'rgba(255, 107, 107, 0.15)',
  // 次要色 - 琥珀黄
  secondary: '#FFB347',
  // 第三色 - 青绿色
  tertiary: '#4ECDC4',
  // 第四色 - 紫罗兰
  quaternary: '#A78BFA',
  // 第五色 - 天蓝色
  quinary: '#60A5FA',
}

// 图表主题相关颜色
const chartThemeColors = computed(() => ({
  text: isDark.value ? 'rgba(255, 255, 255, 0.85)' : 'rgba(0, 0, 0, 0.85)',
  textSecondary: isDark.value ? 'rgba(255, 255, 255, 0.55)' : 'rgba(0, 0, 0, 0.55)',
  textTertiary: isDark.value ? 'rgba(255, 255, 255, 0.35)' : 'rgba(0, 0, 0, 0.35)',
  borderColor: isDark.value ? 'rgba(255, 255, 255, 0.1)' : 'rgba(0, 0, 0, 0.1)',
  splitLineColor: isDark.value ? 'rgba(255, 255, 255, 0.06)' : 'rgba(0, 0, 0, 0.06)',
  tooltipBg: isDark.value ? 'rgba(40, 40, 40, 0.95)' : 'rgba(255, 255, 255, 0.8)',
  tooltipShadow: isDark.value ? 'rgba(0, 0, 0, 0.4)' : 'rgba(0, 0, 0, 0.06)',
  crosshairColor: isDark.value ? 'rgba(255, 255, 255, 0.15)' : 'rgba(0, 0, 0, 0.1)',
}))

// 通用 Tooltip 配置
const baseTooltipConfig = computed(() => ({
  trigger: 'axis' as const,
  confine: false,
  backgroundColor: chartThemeColors.value.tooltipBg,
  borderColor: 'transparent',
  borderWidth: 0,
  borderRadius: 6,
  textStyle: {
    color: chartThemeColors.value.text,
    fontSize: 12,
    lineHeight: 20,
  },
  extraCssText: `${appStore.backgroundEnabled ? 'backdrop-filter: blur(5px);' : ''}z-index:9;box-shadow:0 0 0 1px ${chartThemeColors.value.tooltipShadow}, 0 0 16px ${chartThemeColors.value.tooltipShadow}`,
  axisPointer: {
    type: 'cross' as const,
    crossStyle: {
      color: chartThemeColors.value.textTertiary,
    },
    lineStyle: {
      color: chartThemeColors.value.crosshairColor,
      width: 1,
      type: 'dashed' as const,
    },
    shadowStyle: {
      color: chartThemeColors.value.crosshairColor,
    },
  },
}))

// 图表边距配置
const chartMargin = { top: 30, right: 24, bottom: 32, left: 56 }
const chartMarginWithLegend = { top: 30, right: 24, bottom: 52, left: 56 }

// Service 完整覆盖所选时间窗并有界降采样；UI 最多展示 90 天以控制浏览器内存。
const presetViews = [
  { label: '1 小时', hours: 1 },
  { label: '6 小时', hours: 6 },
  { label: '1 天', hours: 24 },
  { label: '7 天', hours: 168 },
  { label: '30 天', hours: 720 },
  { label: '60 天', hours: 1440 },
  { label: '90 天', hours: 2160 },
]

// 可用视图列表
const availableViews = computed(() => {
  const views: { label: string, hours?: number }[] = [{ label: '实时' }]
  const maxHours = maxRecordPreserveTime.value

  for (const v of presetViews) {
    if (maxHours >= v.hours) {
      views.push({ label: v.label, hours: v.hours })
    }
  }

  return views
})

// 当前选中的视图
const selectedView = ref<string>('实时')
const selectedHours = computed(() => {
  const view = availableViews.value.find(v => v.label === selectedView.value)
  return view?.hours
})
const isRealtime = computed(() => selectedView.value === '实时')

// 数据状态
const remoteData = shallowRef<StatusRecord[]>([])
const coverage = shallowRef<HistoryCoverage | null>(null)
const loading = ref(false)
const isInitialLoad = ref(true) // 是否为首次加载（用于控制实时模式下的 NSpin 显示）
const error = ref<string | null>(null)

// 节点信息
const nodeInfo = computed(() => nodesStore.nodesByUuid.get(props.uuid))

// RPC 客户端
const rpc = getSharedRpc()
let activeRequest: AbortController | null = null
let requestGeneration = 0

// ==================== 数据获取 ====================

function statusToRecordFormat(records: StatusRecord[]): RecordFormat[] {
  return records.map(r => ({
    client: r.client,
    time: r.time,
    cpu: r.cpu ?? null,
    gpu: r.gpu ?? null,
    gpu_usage: null,
    gpu_memory: null,
    ram: r.ram ?? null,
    ram_total: r.ram_total ?? null,
    swap: r.swap ?? null,
    swap_total: r.swap_total ?? null,
    load: r.load ?? null,
    temp: r.temp ?? null,
    disk: r.disk ?? null,
    disk_total: r.disk_total ?? null,
    net_in: r.net_in ?? null,
    net_out: r.net_out ?? null,
    net_total_up: r.net_total_up ?? null,
    net_total_down: r.net_total_down ?? null,
    process: r.process ?? null,
    connections: r.connections ?? null,
    connections_udp: r.connections_udp ?? null,
  }))
}

async function fetchData() {
  activeRequest?.abort()
  const uuid = props.uuid
  if (!uuid) {
    requestGeneration++
    activeRequest = null
    loading.value = false
    return
  }

  const controller = new AbortController()
  activeRequest = controller
  const generation = ++requestGeneration
  const realtime = isRealtime.value
  const hours = selectedHours.value || 4
  if (!realtime || isInitialLoad.value)
    loading.value = true
  error.value = null

  try {
    const result = realtime
      ? await rpc.getNodeRecentStatus(uuid, 150, controller.signal)
      : await rpc.getLoadRecords(uuid, hours, 1_000, controller.signal)
    if (generation !== requestGeneration)
      return
    const records = result.records || []
    records.sort((a: StatusRecord, b: StatusRecord) =>
      dayjs(a.time).valueOf() - dayjs(b.time).valueOf(),
    )
    remoteData.value = records
    coverage.value = result.coverage
  }
  catch (err) {
    if (controller.signal.aborted || generation !== requestGeneration)
      return
    error.value = err instanceof Error ? err.message : '获取数据失败'
    remoteData.value = []
    coverage.value = null
  }
  finally {
    if (generation === requestGeneration) {
      activeRequest = null
      loading.value = false
      isInitialLoad.value = false
    }
  }
}

// ==================== 数据处理 ====================

const chartData = computed(() => {
  const data = statusToRecordFormat(remoteData.value)
  if (isRealtime.value) {
    return data
  }

  const requestedWindow = coverage.value
  if (!requestedWindow)
    return data

  const hours = selectedHours.value || 4
  const minute = 60
  const hour = minute * 60
  let intervalSec: number

  if (hours <= 4) {
    intervalSec = minute
  }
  else if (hours > 120) {
    intervalSec = hour
  }
  else {
    intervalSec = minute * 15
  }

  const intervalMs = Math.max(intervalSec * 1_000, requestedWindow.bucket_ms)
  const emptyTemplate: RecordFormat = {
    client: props.uuid,
    time: '',
    cpu: null,
    gpu: null,
    gpu_usage: null,
    gpu_memory: null,
    ram: null,
    ram_total: null,
    swap: null,
    swap_total: null,
    load: null,
    temp: null,
    disk: null,
    disk_total: null,
    net_in: null,
    net_out: null,
    net_total_up: null,
    net_total_down: null,
    process: null,
    connections: null,
    connections_udp: null,
  }
  return alignRecordsToTimeWindow(
    data,
    intervalMs,
    requestedWindow.requested_start_unix_ms,
    requestedWindow.requested_end_unix_ms,
    emptyTemplate,
  )
})

const latestStatus = computed(() => {
  const data = remoteData.value
  if (!data.length)
    return null
  return data.at(-1) ?? null
})

const hasConnectionData = computed(() => remoteData.value.some(record =>
  record.connections != null || record.connections_udp != null,
))
const hasProcessData = computed(() => remoteData.value.some(record => record.process != null))
const tableRows = computed(() => remoteData.value.slice(-100))
const chartSummary = computed(() => {
  const latest = latestStatus.value
  if (!latest)
    return '当前没有可用的节点历史数据。'
  const range = coverage.value
    ? `${new Date(coverage.value.requested_start_unix_ms).toLocaleString()} 至 ${new Date(coverage.value.requested_end_unix_ms).toLocaleString()}`
    : '当前时间窗'
  return `${range}；最新 CPU ${latest.cpu.toFixed(1)}%，1 分钟负载 ${latest.load.toFixed(2)}，内存 ${formatBytes(latest.ram)}，磁盘 ${formatBytes(latest.disk)}。`
})

function chartAria(description: string) {
  return { enabled: true, description: `${description}。${chartSummary.value}` }
}

// ==================== 工具函数 ====================

function formatTime(time: string, showDate: boolean): string {
  const date = dayjs(time)
  if (showDate) {
    return date.format('M/D HH:mm')
  }
  return date.format(isRealtime.value ? 'HH:mm:ss' : 'HH:mm')
}

function formatTimeForTooltip(time: string, hours: number): string {
  const date = dayjs(time)
  if (hours < 24) {
    return date.format('HH:mm:ss')
  }
  return date.format('MM/DD HH:mm')
}

const showDateInAxis = computed(() => (selectedHours.value || 1) >= 24)

// 通用 X 轴配置
const baseXAxisConfig = computed(() => ({
  type: 'category' as const,
  data: chartData.value.map(r => formatTime(r.time, showDateInAxis.value)),
  axisLabel: {
    fontSize: 11,
    color: chartThemeColors.value.textSecondary,
    margin: 12,
    hideOverlap: true,
  },
  axisLine: {
    show: true,
    lineStyle: { color: chartThemeColors.value.borderColor, width: 1 },
  },
  axisTick: { show: false },
  boundaryGap: false,
}))

// 通用 Y 轴配置
const baseYAxisConfig = computed(() => ({
  type: 'value' as const,
  axisLabel: {
    fontSize: 11,
    color: chartThemeColors.value.textSecondary,
  },
  axisLine: { show: false },
  axisTick: { show: false },
  splitLine: {
    lineStyle: {
      color: chartThemeColors.value.splitLineColor,
      type: 'dashed' as const,
    },
  },
  axisPointer: {
    lineStyle: { opacity: 0 },
    crossStyle: { opacity: 0 },
    label: { show: false },
  },
}))

// ==================== 图表配置 ====================

// CPU 图表
const cpuChartOption = computed(() => ({
  animation: false,
  aria: chartAria('CPU 使用率和系统负载时间序列'),
  // 全局颜色配置（确保 Tooltip 圆点颜色与线条一致）
  color: [chartColors.primary, chartColors.secondary],
  tooltip: {
    ...baseTooltipConfig.value,
    formatter: (params: unknown) => {
      const p = params as Array<{ dataIndex: number, seriesName: string, value: number, color: string }>
      if (!p.length)
        return ''
      const firstParam = p[0]
      if (!firstParam)
        return ''
      const record = chartData.value[firstParam.dataIndex]
      if (!record)
        return ''

      const timeStr = formatTimeForTooltip(record.time, selectedHours.value || 1)
      let html = `<div style="font-weight:600;margin-bottom:6px;color:${chartThemeColors.value.textSecondary}">${timeStr}</div>`
      html += '<div style="display:flex;flex-direction:column;gap:4px">'

      for (const item of p) {
        const colorDot = `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${item.color};margin-right:8px;flex-shrink:0"></span>`
        if (item.seriesName === 'CPU') {
          html += `<div style="display:flex;align-items:center">${colorDot}<span>CPU</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${item.value?.toFixed(1) ?? '-'}%</span></div>`
        }
        else if (item.seriesName === '负载') {
          html += `<div style="display:flex;align-items:center">${colorDot}<span>系统负载</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${item.value?.toFixed(2) ?? '-'}</span></div>`
        }
      }
      html += '</div>'
      return html
    },
  },
  grid: chartMargin,
  xAxis: baseXAxisConfig.value,
  yAxis: [
    {
      ...baseYAxisConfig.value,
      name: 'CPU %',
      nameTextStyle: { color: chartThemeColors.value.textSecondary, padding: [0, 40, 0, 0] },
      min: 0,
      max: 100,
      axisLabel: { ...baseYAxisConfig.value.axisLabel, formatter: '{value}%' },
    },
    {
      ...baseYAxisConfig.value,
      name: '负载',
      nameTextStyle: { color: chartThemeColors.value.textSecondary, padding: [0, 0, 0, 40] },
      min: 0,
      splitLine: { show: false },
    },
  ],
  series: [
    {
      name: 'CPU',
      type: 'line',
      data: chartData.value.map(r => r.cpu),

      showSymbol: false,
      yAxisIndex: 0,
      lineStyle: { width: 1.5, color: chartColors.primary, cap: 'round' as const },
      areaStyle: {
        color: {
          type: 'linear',
          x: 0,
          y: 0,
          x2: 0,
          y2: 1,
          colorStops: [
            { offset: 0, color: 'rgba(255, 107, 107, 0.25)' },
            { offset: 1, color: 'rgba(255, 107, 107, 0.02)' },
          ],
        },
      },
    },
    {
      name: '负载',
      type: 'line',
      data: chartData.value.map(r => r.load),

      showSymbol: false,
      yAxisIndex: 1,
      lineStyle: { width: 1.5, color: chartColors.secondary, cap: 'round' as const },
    },
  ],
}))

// 内存图表
const memoryChartOption = computed(() => ({
  animation: false,
  aria: chartAria('内存和交换空间使用量时间序列'),
  color: [chartColors.primary, chartColors.secondary],
  tooltip: {
    ...baseTooltipConfig.value,
    formatter: (params: unknown) => {
      const p = params as Array<{ dataIndex: number, seriesName: string, value: number, color: string }>
      if (!p.length)
        return ''
      const firstParam = p[0]
      if (!firstParam)
        return ''
      const record = chartData.value[firstParam.dataIndex]
      if (!record)
        return ''

      const ramUsed = record.ram
      const ramTotal = record.ram_total ?? nodeInfo.value?.mem_total ?? null
      const swapUsed = record.swap
      const swapTotal = record.swap_total ?? nodeInfo.value?.swap_total ?? null
      const ramDisplay = ramUsed == null ? '-' : formatBytes(ramUsed)
      const swapDisplay = swapUsed == null ? '-' : formatBytes(swapUsed)
      const ramPercent = ramUsed != null && ramTotal != null && ramTotal > 0
        ? ` (${((ramUsed / ramTotal) * 100).toFixed(1)}%)`
        : ''
      const swapPercent = swapUsed != null && swapTotal != null && swapTotal > 0
        ? ` (${((swapUsed / swapTotal) * 100).toFixed(1)}%)`
        : ''

      const timeStr = formatTimeForTooltip(record.time, selectedHours.value || 1)
      let html = `<div style="font-weight:600;margin-bottom:6px;color:${chartThemeColors.value.textSecondary}">${timeStr}</div>`
      html += '<div style="display:flex;flex-direction:column;gap:4px">'

      for (const item of p) {
        const colorDot = `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${item.color};margin-right:8px;flex-shrink:0"></span>`
        if (item.seriesName === 'RAM') {
          html += `<div style="display:flex;align-items:center">${colorDot}<span>RAM</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${ramDisplay}${ramPercent}</span></div>`
        }
        else if (item.seriesName === 'Swap') {
          html += `<div style="display:flex;align-items:center">${colorDot}<span>Swap</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${swapDisplay}${swapPercent}</span></div>`
        }
      }
      html += '</div>'
      return html
    },
  },
  grid: chartMargin,
  xAxis: baseXAxisConfig.value,
  yAxis: {
    ...baseYAxisConfig.value,
    name: '内存',
    nameTextStyle: { color: chartThemeColors.value.textSecondary, padding: [0, 40, 0, 0] },
    axisLabel: {
      ...baseYAxisConfig.value.axisLabel,
      formatter: (val: number) => formatBytes(val),
    },
  },
  series: [
    {
      name: 'RAM',
      type: 'line',
      data: chartData.value.map(r => r.ram),

      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.primary, cap: 'round' as const },
      areaStyle: {
        color: {
          type: 'linear',
          x: 0,
          y: 0,
          x2: 0,
          y2: 1,
          colorStops: [
            { offset: 0, color: 'rgba(255, 107, 107, 0.25)' },
            { offset: 1, color: 'rgba(255, 107, 107, 0.02)' },
          ],
        },
      },
    },
    {
      name: 'Swap',
      type: 'line',
      data: chartData.value.map(r => r.swap),

      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.secondary, cap: 'round' as const },
    },
  ],
}))

// 磁盘图表
const diskChartOption = computed(() => ({
  animation: false,
  aria: chartAria('磁盘使用量时间序列'),
  color: [chartColors.tertiary],
  tooltip: {
    ...baseTooltipConfig.value,
    formatter: (params: unknown) => {
      const p = params as Array<{ dataIndex: number, value: number, color: string }>
      if (!p.length)
        return ''
      const firstParam = p[0]
      if (!firstParam)
        return ''
      const record = chartData.value[firstParam.dataIndex]
      if (!record)
        return ''

      const diskUsed = record.disk
      const diskTotal = record.disk_total ?? nodeInfo.value?.disk_total ?? null
      const diskDisplay = diskUsed == null ? '-' : formatBytes(diskUsed)
      const diskPercent = diskUsed != null && diskTotal != null && diskTotal > 0
        ? ` (${((diskUsed / diskTotal) * 100).toFixed(1)}%)`
        : ''

      const timeStr = formatTimeForTooltip(record.time, selectedHours.value || 1)
      const colorDot = `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${firstParam.color};margin-right:8px;flex-shrink:0"></span>`

      let html = `<div style="font-weight:600;margin-bottom:6px;color:${chartThemeColors.value.textSecondary}">${timeStr}</div>`
      html += '<div style="display:flex;flex-direction:column;gap:4px">'
      html += `<div style="display:flex;align-items:center">${colorDot}<span>磁盘已用</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${diskDisplay}${diskPercent}</span></div>`
      html += '</div>'
      return html
    },
  },
  grid: chartMargin,
  xAxis: baseXAxisConfig.value,
  yAxis: {
    ...baseYAxisConfig.value,
    name: '磁盘',
    nameTextStyle: { color: chartThemeColors.value.textSecondary, padding: [0, 40, 0, 0] },
    axisLabel: {
      ...baseYAxisConfig.value.axisLabel,
      formatter: (val: number) => formatBytes(val),
    },
  },
  series: [
    {
      name: '磁盘已用',
      type: 'line',
      data: chartData.value.map(r => r.disk),
      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.tertiary, cap: 'round' as const },
      areaStyle: {
        color: {
          type: 'linear',
          x: 0,
          y: 0,
          x2: 0,
          y2: 1,
          colorStops: [
            { offset: 0, color: 'rgba(78, 205, 196, 0.25)' },
            { offset: 1, color: 'rgba(78, 205, 196, 0.02)' },
          ],
        },
      },
    },
  ],
}))

// 网络图表
const networkChartOption = computed(() => ({
  animation: false,
  aria: chartAria('网络上传和下载速率时间序列'),
  color: [chartColors.quinary, chartColors.quaternary],
  tooltip: {
    ...baseTooltipConfig.value,
    formatter: (params: unknown) => {
      const p = params as Array<{ dataIndex: number, seriesName: string, value: number, color: string }>
      if (!p.length)
        return ''
      const firstParam = p[0]
      if (!firstParam)
        return ''
      const record = chartData.value[firstParam.dataIndex]
      if (!record)
        return ''

      const timeStr = formatTimeForTooltip(record.time, selectedHours.value || 1)
      let html = `<div style="font-weight:600;margin-bottom:6px;color:${chartThemeColors.value.textSecondary}">${timeStr}</div>`
      html += '<div style="display:flex;flex-direction:column;gap:4px">'

      for (const item of p) {
        const colorDot = `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${item.color};margin-right:8px;flex-shrink:0"></span>`
        const label = item.seriesName === '下载' ? '↓ 下载' : '↑ 上传'
        const displayValue = item.value == null ? '-' : `${formatBytes(item.value)}/s`
        html += `<div style="display:flex;align-items:center">${colorDot}<span>${label}</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${displayValue}</span></div>`
      }
      html += '</div>'
      return html
    },
  },
  legend: {
    data: ['下载', '上传'],
    bottom: 4,
    itemWidth: 12,
    itemHeight: 12,
    itemGap: 20,
    icon: 'roundRect',
    textStyle: { fontSize: 11, color: chartThemeColors.value.textSecondary },
  },
  grid: chartMarginWithLegend,
  xAxis: baseXAxisConfig.value,
  yAxis: {
    ...baseYAxisConfig.value,
    name: '速度',
    nameTextStyle: { color: chartThemeColors.value.textSecondary, padding: [0, 40, 0, 0] },
    axisLabel: {
      ...baseYAxisConfig.value.axisLabel,
      formatter: (val: number) => `${formatBytes(val)}/s`,
    },
  },
  series: [
    {
      name: '下载',
      type: 'line',
      data: chartData.value.map(r => r.net_in),

      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.quinary, cap: 'round' as const },
    },
    {
      name: '上传',
      type: 'line',
      data: chartData.value.map(r => r.net_out),

      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.quaternary, cap: 'round' as const },
    },
  ],
}))

// 连接数图表
const connectionsChartOption = computed(() => ({
  animation: false,
  aria: chartAria('TCP 和 UDP 连接数时间序列'),
  color: [chartColors.primary, chartColors.tertiary],
  tooltip: {
    ...baseTooltipConfig.value,
    formatter: (params: unknown) => {
      const p = params as Array<{ dataIndex: number, seriesName: string, value: number, color: string }>
      if (!p.length)
        return ''
      const firstParam = p[0]
      if (!firstParam)
        return ''
      const record = chartData.value[firstParam.dataIndex]
      if (!record)
        return ''

      const timeStr = formatTimeForTooltip(record.time, selectedHours.value || 1)
      let html = `<div style="font-weight:600;margin-bottom:6px;color:${chartThemeColors.value.textSecondary}">${timeStr}</div>`
      html += '<div style="display:flex;flex-direction:column;gap:4px">'

      for (const item of p) {
        const colorDot = `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${item.color};margin-right:8px;flex-shrink:0"></span>`
        const displayValue = item.value != null ? Math.round(item.value) : '-'
        html += `<div style="display:flex;align-items:center">${colorDot}<span>${item.seriesName}</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${displayValue}</span></div>`
      }
      html += '</div>'
      return html
    },
  },
  legend: {
    data: ['TCP', 'UDP'],
    bottom: 4,
    itemWidth: 12,
    itemHeight: 12,
    itemGap: 20,
    icon: 'roundRect',
    textStyle: { fontSize: 11, color: chartThemeColors.value.textSecondary },
  },
  grid: chartMarginWithLegend,
  xAxis: baseXAxisConfig.value,
  yAxis: {
    ...baseYAxisConfig.value,
    name: '连接数',
    nameTextStyle: { color: chartThemeColors.value.textSecondary, padding: [0, 40, 0, 0] },
    min: 0,
    axisLabel: {
      ...baseYAxisConfig.value.axisLabel,
      formatter: (val: number) => Math.round(val).toString(),
    },
  },
  series: [
    {
      name: 'TCP',
      type: 'line',
      data: chartData.value.map(r => r.connections),

      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.primary, cap: 'round' as const },
    },
    {
      name: 'UDP',
      type: 'line',
      data: chartData.value.map(r => r.connections_udp),

      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.tertiary, cap: 'round' as const },
    },
  ],
}))

// 进程数图表
const processChartOption = computed(() => ({
  animation: false,
  aria: chartAria('进程数时间序列'),
  color: [chartColors.quaternary],
  tooltip: {
    ...baseTooltipConfig.value,
    formatter: (params: unknown) => {
      const p = params as Array<{ dataIndex: number, value: number, color: string }>
      if (!p.length)
        return ''
      const firstParam = p[0]
      if (!firstParam)
        return ''
      const record = chartData.value[firstParam.dataIndex]
      if (!record)
        return ''

      const timeStr = formatTimeForTooltip(record.time, selectedHours.value || 1)
      const colorDot = `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${firstParam.color};margin-right:8px;flex-shrink:0"></span>`
      const displayValue = firstParam.value != null ? Math.round(firstParam.value) : '-'

      let html = `<div style="font-weight:600;margin-bottom:6px;color:${chartThemeColors.value.textSecondary}">${timeStr}</div>`
      html += '<div style="display:flex;flex-direction:column;gap:4px">'
      html += `<div style="display:flex;align-items:center">${colorDot}<span>进程数</span><span style="margin-left:auto;font-weight:600;margin-left:16px">${displayValue}</span></div>`
      html += '</div>'
      return html
    },
  },
  grid: chartMargin,
  xAxis: baseXAxisConfig.value,
  yAxis: {
    ...baseYAxisConfig.value,
    name: '进程',
    nameTextStyle: { color: chartThemeColors.value.textSecondary, padding: [0, 40, 0, 0] },
    min: 0,
    axisLabel: {
      ...baseYAxisConfig.value.axisLabel,
      formatter: (val: number) => Math.round(val).toString(),
    },
  },
  series: [
    {
      name: '进程数',
      type: 'line',
      data: chartData.value.map(r => r.process),

      showSymbol: false,
      lineStyle: { width: 1.5, color: chartColors.quaternary, cap: 'round' as const },
      areaStyle: {
        color: {
          type: 'linear',
          x: 0,
          y: 0,
          x2: 0,
          y2: 1,
          colorStops: [
            { offset: 0, color: 'rgba(167, 139, 250, 0.25)' },
            { offset: 1, color: 'rgba(167, 139, 250, 0.02)' },
          ],
        },
      },
    },
  ],
}))

// ==================== 实时更新 ====================

// 使用 VueUse 的 useIntervalFn 自动管理定时器
const { pause: pauseRealtimeUpdate, resume: resumeRealtimeUpdate } = useIntervalFn(
  () => fetchData(),
  dataUpdateInterval,
  { immediate: false },
)

// 根据是否为实时模式控制定时器
watch(isRealtime, (realtime) => {
  if (realtime) {
    resumeRealtimeUpdate()
  }
  else {
    pauseRealtimeUpdate()
  }
}, { immediate: true })

// 生命周期 ====================

watch(selectedView, () => {
  isInitialLoad.value = true // 切换视图时重置首次加载状态
  fetchData()
})

watch(() => props.uuid, () => {
  remoteData.value = []
  coverage.value = null
  isInitialLoad.value = true // 切换节点时重置首次加载状态
  fetchData()
})

onMounted(() => {
  fetchData()
})

onBeforeUnmount(() => {
  requestGeneration++
  activeRequest?.abort()
  activeRequest = null
})
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- 时间选择器 -->
    <Tabs v-model="selectedView" class="w-full items-center">
      <TabsList :class="pickSurfaceClass('h-8 bg-background/60 pointer-events-auto rounded-md', 'h-8 bg-background/50 backdrop-blur-xl pointer-events-auto rounded-md')">
        <TabsTrigger
          v-for="view in availableViews" :key="view.label" :value="view.label"
          class="h-6.5 text-xs border-none data-[state=active]:text-emerald-700 dark:data-[state=active]:text-emerald-600 shadow-none rounded-sm"
        >
          {{ view.label }}
        </TabsTrigger>
      </TabsList>
    </Tabs>

    <!-- 内容区域 -->
    <Spinner :show="loading">
      <p class="sr-only" role="status">
        {{ chartSummary }}
      </p>
      <p
        v-if="coverage?.downsampled"
        class="mb-3 text-center text-[11px] text-muted-foreground"
      >
        已将 {{ coverage.source_points.toLocaleString() }} 个采样点按完整时间范围聚合为
        {{ coverage.returned_points.toLocaleString() }} 个点
      </p>
      <div v-if="error" class="text-red-500 py-8 text-center">
        {{ error }}
      </div>
      <div v-else-if="remoteData.length === 0 && !loading" class="py-8">
        <Empty description="暂无负载数据" />
      </div>

      <!-- 图表网格 -->
      <div v-else class="gap-4 grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3">
        <!-- CPU 卡片 -->
        <CardX
          size="small"
          class="border-none transition-all rounded-md"
          :class="pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs')"
        >
          <template #header>
            <div class="flex items-center justify-between">
              <span class="text-base font-bold">CPU</span>
              <div v-if="latestStatus?.cpu != null" class="text-xs flex gap-0.5 items-baseline">
                <span>{{ latestStatus.cpu.toFixed(1) }}</span>
                <span>%</span>
              </div>
              <span v-else>-</span>
            </div>
          </template>
          <div class="h-48">
            <VChart :option="cpuChartOption" autoresize />
          </div>
        </CardX>

        <!-- 内存卡片 -->
        <CardX
          size="small"
          class="border-none transition-all rounded-md"
          :class="pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs')"
        >
          <template #header>
            <div class="flex items-center justify-between">
              <span class="text-base font-bold">内存</span>
              <div class="text-xs flex gap-1 items-baseline">
                <template v-if="latestStatus?.ram != null">
                  <span>{{ formatBytesSplit(latestStatus.ram).value }}</span>
                  <span>{{ formatBytesSplit(latestStatus.ram).unit }}</span>
                </template>
                <span v-else>-</span>
                <span>·</span>
                <template v-if="nodeInfo?.mem_total">
                  <span>{{
                    formatBytesSplit(nodeInfo.mem_total).value }}</span>
                  <span>{{ formatBytesSplit(nodeInfo.mem_total).unit }}</span>
                </template>
                <span v-else>-</span>
              </div>
            </div>
          </template>
          <div class="h-48">
            <VChart :option="memoryChartOption" autoresize />
          </div>
        </CardX>

        <!-- 磁盘卡片 -->
        <CardX
          size="small"
          class="border-none transition-all rounded-md"
          :class="pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs')"
        >
          <template #header>
            <div class="flex items-center justify-between">
              <span class="text-base font-bold">磁盘</span>
              <div class="text-xs flex gap-1 items-baseline">
                <template v-if="latestStatus?.disk != null">
                  <span>{{ formatBytesSplit(latestStatus.disk).value }}</span>
                  <span>{{ formatBytesSplit(latestStatus.disk).unit }}</span>
                </template>
                <span v-else>-</span>
                <span>·</span>
                <template v-if="nodeInfo?.disk_total">
                  <span>{{ formatBytesSplit(nodeInfo.disk_total).value }}</span>
                  <span>{{ formatBytesSplit(nodeInfo.disk_total).unit }}</span>
                </template>
                <span v-else>-</span>
              </div>
            </div>
          </template>
          <div class="h-48">
            <VChart :option="diskChartOption" autoresize />
          </div>
        </CardX>

        <!-- 网络卡片 -->
        <CardX
          size="small"
          class="border-none transition-all rounded-md"
          :class="pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs')"
        >
          <template #header>
            <div class="flex items-center justify-between">
              <span class="text-base font-bold">网络</span>
              <div class="text-xs flex gap-2 items-baseline">
                <span class="flex flex-row items-center justify-center gap-0.5">
                  <Icon icon="tabler:chevron-up" width="12" height="12" />
                  <template v-if="latestStatus?.net_out != null">
                    {{ formatBytesSplit(latestStatus.net_out).value }}
                    {{ formatBytesSplit(latestStatus.net_out).unit }}/s
                  </template>
                  <template v-else>-</template>
                </span>
                <span class="flex flex-row items-center justify-center gap-0.5">
                  <Icon icon="tabler:chevron-down" width="12" height="12" />
                  <template v-if="latestStatus?.net_in != null">
                    {{ formatBytesSplit(latestStatus.net_in).value }}
                    {{ formatBytesSplit(latestStatus.net_in).unit }}/s
                  </template>
                  <template v-else>-</template>
                </span>
              </div>
            </div>
          </template>
          <div class="h-48">
            <VChart :option="networkChartOption" autoresize />
          </div>
        </CardX>

        <!-- 连接数卡片 -->
        <CardX
          v-if="hasConnectionData"
          size="small"
          class="border-none transition-all rounded-md"
          :class="pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs')"
        >
          <template #header>
            <div class="flex items-center justify-between">
              <span class="text-base font-bold">连接</span>
              <div class="text-xs flex gap-1 items-baseline">
                <span>TCP: {{ latestStatus?.connections ?? '-' }}</span>
                <span>·</span>
                <span>UDP: {{ latestStatus?.connections_udp ?? '-' }}</span>
              </div>
            </div>
          </template>
          <div class="h-48">
            <VChart :option="connectionsChartOption" autoresize />
          </div>
        </CardX>

        <!-- 进程卡片 -->
        <CardX
          v-if="hasProcessData"
          size="small"
          class="border-none transition-all rounded-md"
          :class="pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs')"
        >
          <template #header>
            <div class="flex items-center justify-between">
              <span class="text-base font-bold">进程</span>
              <span class="text-xs">
                {{ latestStatus?.process ?? '-' }}
              </span>
            </div>
          </template>
          <div class="h-48">
            <VChart :option="processChartOption" autoresize />
          </div>
        </CardX>
      </div>

      <details v-if="remoteData.length > 0" class="mt-4 rounded-md bg-background/50 p-3 text-xs">
        <summary class="min-h-6 cursor-pointer font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
          查看最近 {{ tableRows.length }} 个采样点的数据表
        </summary>
        <div class="mt-3 overflow-x-auto">
          <table class="w-full min-w-180 border-collapse text-left tabular-nums">
            <caption class="sr-only">
              {{ chartSummary }}
            </caption>
            <thead>
              <tr class="text-muted-foreground">
                <th scope="col" class="p-2">
                  时间
                </th>
                <th scope="col" class="p-2">
                  CPU
                </th>
                <th scope="col" class="p-2">
                  负载
                </th>
                <th scope="col" class="p-2">
                  内存
                </th>
                <th scope="col" class="p-2">
                  磁盘
                </th>
                <th scope="col" class="p-2">
                  上传
                </th>
                <th scope="col" class="p-2">
                  下载
                </th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="record in tableRows" :key="record.time" class="border-t border-border/50">
                <td class="whitespace-nowrap p-2">
                  {{ new Date(record.time).toLocaleString() }}
                </td>
                <td class="p-2">
                  {{ record.cpu.toFixed(1) }}%
                </td>
                <td class="p-2">
                  {{ record.load.toFixed(2) }}
                </td>
                <td class="p-2">
                  {{ formatBytes(record.ram) }}
                </td>
                <td class="p-2">
                  {{ formatBytes(record.disk) }}
                </td>
                <td class="p-2">
                  {{ formatBytes(record.net_out) }}/s
                </td>
                <td class="p-2">
                  {{ formatBytes(record.net_in) }}/s
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </details>
    </Spinner>
  </div>
</template>
