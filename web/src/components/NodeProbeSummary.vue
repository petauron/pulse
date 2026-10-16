<script setup lang="ts">
import type { NodeProbe } from '@/utils/rpc'
import { computed } from 'vue'
import { DataTooltip } from '@/components/ui/data-tooltip'
import { useAppStore } from '@/stores/app'
import { getProbeToneColor, selectHomeProbes } from '@/utils/nodeHelpers'

const props = defineProps<{ probes?: NodeProbe[], compact?: boolean, nodeUuid?: string, interactive?: boolean, nodeName?: string }>()
const emit = defineEmits<{ open: [] }>()
const app = useAppStore()
const metrics = ['latency', 'loss'] as const
const historyHours = computed(() => {
  if (!props.nodeUuid)
    return 1
  const configured = app.publicSettings?.theme_settings?.homePingHistoryHours
  const requested = typeof configured === 'number' && Number.isFinite(configured) ? Math.round(configured) : 1
  return Math.min(168, app.publicSettings?.ping_record_preserve_time ?? 168, Math.max(1, requested))
})
const historyLabel = computed(() => historyHours.value % 24 === 0 ? `${historyHours.value / 24} 天` : `${historyHours.value} 小时`)
const emptyPoints: NodeProbe['points'] = Array.from({ length: 10 }, (_, bucket) => ({ bucket, time: 0, latency: null, loss: 0, samples: 0, successful_samples: 0 }))
const rows = computed(() => {
  const selected = props.nodeUuid
    ? selectHomeProbes(props.probes ?? [], props.nodeUuid, app.publicSettings?.theme_settings)
    : props.probes ?? []
  return selected.map((task, index) => {
    if (!task) {
      return { id: `empty-${index}`, name: '未配置', kind: 'icmp' as const, stale: true, points: emptyPoints, latency: '—', loss: '—' }
    }
    const stale = !task.points.some(point => point.samples > 0)
    const points = task.points.slice(-10)
    const successes = points.filter(p => p.successful_samples > 0 && p.latency !== null)
    return {
      id: task.id,
      name: task.name,
      kind: task.kind,
      stale,
      points: emptyPoints.map((empty, bucket) => points.find(p => p.bucket === bucket) ?? empty),
      latency: !stale && successes.length ? `${Math.round(successes.reduce((sum, p) => sum + p.latency! * p.successful_samples, 0) / successes.reduce((sum, p) => sum + p.successful_samples, 0))} ms` : '—',
      loss: !stale && points.length ? `${(points.reduce((sum, p) => sum + p.loss * p.samples, 0) / points.reduce((sum, p) => sum + p.samples, 0)).toFixed(1)}%` : '—',
    }
  })
})
function sampleTooltip(name: string, point: NodeProbe['points'][number], metric: typeof metrics[number]): string {
  if (!point.samples)
    return `${name} · 此时间段无样本`
  const result = metric === 'loss'
    ? `${point.loss.toFixed(1)}% · ${point.samples} 次探测`
    : point.latency === null ? '无成功样本' : `${Math.round(point.latency)} ms · ${point.successful_samples} 次成功探测`
  const time = historyHours.value >= 24 ? new Date(point.time).toLocaleString() : new Date(point.time).toLocaleTimeString()
  return `${name} · ${time} · ${result}`
}
function panelTooltip(row: { name: string, kind: NodeProbe['kind'], stale: boolean, latency: string, loss: string }, metric: typeof metrics[number]): string {
  if (row.stale)
    return `${row.name} · 暂无数据`
  if (metric === 'latency')
    return `${row.name} · 最近 ${historyLabel.value}平均延迟 ${row.latency}`
  return `${row.name} · 最近 ${historyLabel.value}平均丢包 ${row.loss}${row.kind === 'icmp' ? '' : '（TCP/HTTP 探测失败率）'}`
}
function openMetric(event: Event) {
  if (!props.interactive)
    return
  event.stopPropagation()
  emit('open')
}
function onMetricKeydown(event: KeyboardEvent) {
  if (!props.interactive || (event.key !== 'Enter' && event.key !== ' '))
    return
  event.preventDefault()
  openMetric(event)
}
</script>

<template>
  <div v-if="!compact" class="flex flex-col gap-y-2 text-[11px]" :aria-label="`最近 ${historyLabel}的平均延迟与丢包（TCP/HTTP 为探测失败率），每格约 ${historyHours * 6} 分钟`">
    <div v-for="row in rows" :key="row.id" class="grid h-8 grid-cols-[minmax(64px,0.75fr)_minmax(0,1fr)_minmax(0,1fr)] items-center gap-x-2">
      <DataTooltip placement="top" :content="row.name" class="min-w-0" content-class="min-w-24">
        <span class="block truncate font-medium text-foreground/75">{{ row.name }}</span>
        <span v-if="row.stale" class="sr-only">暂无数据</span>
      </DataTooltip>
      <div
        v-for="metric in metrics" :key="metric" :role="interactive ? 'button' : 'group'" :tabindex="interactive ? 0 : undefined"
        class="group/panel relative flex h-7 min-w-0 flex-col gap-1 text-left"
        :class="interactive ? 'cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring' : undefined"
        :title="interactive ? panelTooltip(row, metric) : undefined"
        :aria-label="interactive ? `${nodeName ?? ''} ${row.name} ${metric === 'latency' ? '延迟' : '丢包'}`.trim() : `${row.name} ${metric === 'latency' ? '延迟' : row.kind === 'icmp' ? '丢包率' : '丢包（TCP/HTTP 探测失败率）'} ${row.stale ? '暂无数据' : row[metric]}`"
        @click="openMetric" @keydown="onMetricKeydown"
      >
        <div class="relative flex items-center justify-between leading-none">
          <span class="shrink-0 text-muted-foreground">{{ metric === 'latency' ? '延迟' : '丢包' }}</span>
          <div class="mx-1 flex-1 border-t-2 border-dotted border-gray-500/10" />
          <span class="shrink-0 font-medium tabular-nums text-foreground/85">{{ row.stale ? '—' : row[metric] }}</span>
        </div>
        <div class="grid h-2 grid-cols-10 items-end gap-px opacity-80 group-hover/panel:opacity-100">
          <DataTooltip
            v-for="(point, i) in row.points" :key="i" placement="top" class="h-full w-full" content-class="w-40 leading-relaxed"
            :content="row.stale ? `${row.name} · 暂无数据` : sampleTooltip(row.name, point, metric)"
          >
            <span
              class="block h-full w-full rounded-[1px] bg-muted-foreground/15 transition-transform duration-150 group-hover/data-tooltip:scale-y-200 motion-reduce:transition-none"
              :style="!point.samples || row.stale ? undefined : { backgroundColor: getProbeToneColor(metric, metric === 'latency' ? point.latency : point.loss, app.publicSettings?.theme_settings), opacity: metric === 'latency' ? 0.9 : 0.86 }"
            />
          </DataTooltip>
        </div>
      </div>
    </div>
    <span v-if="!rows.length" class="text-muted-foreground">未配置探测</span>
  </div>
  <div v-else class="flex h-8 w-full flex-col justify-center gap-px overflow-hidden" aria-label="探测延迟概览，点击查看详情">
    <div v-for="row in rows" :key="row.id" class="grid h-[3px] shrink-0 grid-cols-10 gap-px" :title="`${row.name} · 延迟 ${row.latency} · 丢包 ${row.loss}${row.kind === 'icmp' ? '' : '（探测失败率）'}`">
      <span v-for="(point, i) in row.points" :key="i" class="h-full bg-muted-foreground/15" :style="!point.samples || row.stale ? undefined : { backgroundColor: getProbeToneColor('latency', point.latency, app.publicSettings?.theme_settings) }" />
    </div>
    <span v-if="!rows.length" class="text-[11px] text-muted-foreground">未配置探测</span>
  </div>
</template>
