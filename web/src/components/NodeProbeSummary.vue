<script setup lang="ts">
import type { NodeProbe } from '@/utils/rpc'
import { computed } from 'vue'
import { DataTooltip } from '@/components/ui/data-tooltip'
import { useAppStore } from '@/stores/app'
import { formatDateTime } from '@/utils/helper'
import { getProbeToneColor, selectHomeProbes } from '@/utils/nodeHelpers'
import { failureRateLabel } from '@/utils/probeLabels'

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
      return { id: `empty-${index}`, name: '未配置', kind: 'icmp' as const, stale: true, points: emptyPoints, latency: '--', loss: '--' }
    }
    const stale = task.samples === 0
    const points = task.points.slice(-10)
    return {
      id: task.id,
      name: task.name,
      kind: task.kind,
      stale,
      points: emptyPoints.map((empty, bucket) => points.find(p => p.bucket === bucket) ?? empty),
      latency: task.avg_latency_ms === null ? '--' : `${Math.round(task.avg_latency_ms)} ms`,
      loss: task.loss_percent === null ? '--' : `${task.loss_percent.toFixed(1)}%`,
    }
  })
})
function sampleTooltip(name: string, point: NodeProbe['points'][number], metric: typeof metrics[number]): string {
  const value = !point.samples
    ? 'N/A'
    : metric === 'loss' ? `${point.loss.toFixed(1)}%` : point.latency === null ? 'N/A' : `${Math.round(point.latency)} ms`
  const time = formatDateTime(new Date(point.time), historyHours.value >= 24 ? 'MM-DD HH:mm' : 'HH:mm:ss')
  return `${name}\n${time}\n${value}`
}
function panelTooltip(row: { name: string, kind: NodeProbe['kind'], stale: boolean, latency: string, loss: string }, metric: typeof metrics[number]): string {
  if (row.stale)
    return `${row.name} · 暂无数据`
  if (metric === 'latency')
    return row.latency === '--' ? `${row.name} · 最近 ${historyLabel.value}暂无有效延迟` : `${row.name} · 最近 ${historyLabel.value}平均延迟 ${row.latency}`
  return `${row.name} · 最近 ${historyLabel.value}平均${failureRateLabel(row.kind)} ${row.loss}`
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
  <div v-if="!compact" class="flex flex-col gap-y-2 text-[11px]" :aria-label="`最近 ${historyLabel}的平均延迟与探测结果，每格约 ${historyHours * 6} 分钟`">
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
        :aria-label="interactive ? `${nodeName ?? ''} ${row.name} ${metric === 'latency' ? '延迟' : failureRateLabel(row.kind)}`.trim() : `${row.name} ${metric === 'latency' ? '延迟' : failureRateLabel(row.kind)} ${row.stale ? '暂无数据' : row[metric]}`"
        @click="openMetric" @keydown="onMetricKeydown"
      >
        <div class="relative flex items-center justify-between leading-none">
          <span class="shrink-0 text-muted-foreground">{{ metric === 'latency' ? '延迟' : failureRateLabel(row.kind, true) }}</span>
          <div class="mx-1 flex-1 border-t-2 border-dotted border-gray-500/10" />
          <span class="shrink-0 font-medium tabular-nums text-foreground/85">{{ row.stale ? '--' : row[metric] }}</span>
        </div>
        <div class="grid h-2 grid-cols-10 items-end gap-px opacity-80 group-hover/panel:opacity-100">
          <DataTooltip
            v-for="(point, i) in row.points" :key="i" placement="top" class="h-full w-full"
            :content="row.stale ? `${row.name}\nN/A` : sampleTooltip(row.name, point, metric)"
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
  <!-- On touch, the list cell opens details on the first tap instead of opening a bar tooltip. -->
  <div v-else class="group flex h-8 w-full flex-col justify-center gap-px pr-4" aria-label="首个探测任务的延迟与探测结果概览，点击查看详情" @pointerdown.capture.stop>
    <div v-for="row in rows.slice(0, 1)" :key="row.id" class="flex flex-col gap-px">
      <div v-for="metric in metrics" :key="metric" class="group/panel relative opacity-80 hover:opacity-100" :title="panelTooltip(row, metric)">
        <div :data-probe-metric="metric" class="grid h-1 grid-cols-10 items-end gap-px transition-all hover:h-2.5 motion-reduce:transition-none">
          <DataTooltip
            v-for="(point, i) in row.points" :key="i" placement="top" class="h-full w-full"
            :content="row.stale ? `${row.name}\nN/A` : sampleTooltip(row.name, point, metric)"
          >
            <span
              class="block h-full w-full rounded-[1px] bg-muted-foreground/15 transition-all group-hover/data-tooltip:opacity-100 motion-reduce:transition-none"
              :style="!point.samples || row.stale ? undefined : { backgroundColor: getProbeToneColor(metric, metric === 'latency' ? point.latency : point.loss, app.publicSettings?.theme_settings) }"
            />
          </DataTooltip>
        </div>
      </div>
    </div>
    <span v-if="!rows.length" class="text-[11px] text-muted-foreground">未配置探测</span>
  </div>
</template>
