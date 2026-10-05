<script setup lang="ts">
import type { NodeProbe } from '@/utils/rpc'
import { computed } from 'vue'
import { DataTooltip } from '@/components/ui/data-tooltip'

const props = defineProps<{ probes?: NodeProbe[], compact?: boolean }>()
const metrics = ['latency', 'loss'] as const
const telecomPattern = /电信|telecom/i
const unicomPattern = /联通|unicom/i
const mobilePattern = /移动|mobile/i
function carrierOrder(name: string): number {
  if (telecomPattern.test(name))
    return 0
  if (unicomPattern.test(name))
    return 1
  if (mobilePattern.test(name))
    return 2
  return 3
}
const rows = computed(() => [...(props.probes ?? [])]
  .sort((a, b) => carrierOrder(a.name) - carrierOrder(b.name))
  .slice(0, 3)
  .map((task) => {
    const last = task.points.at(-1)
    const stale = !last || Date.now() - last.time > Math.max(90000, task.interval_seconds * 3000)
    const points = task.points.slice(-10)
    const successes = points.filter(p => p.successful_samples > 0 && p.latency !== null)
    return {
      ...task,
      stale,
      points: Array.from({ length: 10 }, (_, bucket) => points.find(p => p.bucket === bucket) ?? { bucket, time: 0, latency: null, loss: 0, samples: 0, successful_samples: 0 }),
      latency: !stale && successes.length ? `${Math.round(successes.reduce((sum, p) => sum + p.latency! * p.successful_samples, 0) / successes.reduce((sum, p) => sum + p.successful_samples, 0))} ms` : '—',
      loss: !stale && points.length ? `${(points.reduce((sum, p) => sum + p.loss * p.samples, 0) / points.reduce((sum, p) => sum + p.samples, 0)).toFixed(1)}%` : '—',
    }
  }))
// Emerald-Cazi latency scale; keep these thresholds and colors together.
function tone(latency: number | null, success: boolean): string | undefined {
  if (!success)
    return '#F43F5E'
  if (latency === null)
    return undefined
  if (latency <= 60)
    return '#5EEAA6'
  if (latency <= 100)
    return '#47B592'
  if (latency <= 160)
    return '#A3E635'
  if (latency <= 200)
    return '#FACC15'
  return '#F43F5E'
}
function lossTone(loss: number): string {
  if (loss <= 0)
    return '#5EEAA6'
  if (loss < 5)
    return '#A3E635'
  if (loss < 20)
    return '#FACC15'
  return '#F43F5E'
}
function sampleTooltip(name: string, point: NodeProbe['points'][number], metric: typeof metrics[number]): string {
  if (!point.samples)
    return `${name} · 此时间段无样本`
  const result = metric === 'loss'
    ? `${point.loss.toFixed(1)}% · ${point.samples} 次探测`
    : point.latency === null ? '无成功样本' : `${Math.round(point.latency)} ms · ${point.successful_samples} 次成功探测`
  return `${name} · ${new Date(point.time).toLocaleTimeString()} · ${result}`
}
</script>

<template>
  <div v-if="!compact" class="flex flex-col gap-y-2 text-[11px]" aria-label="最近 1 小时的平均延迟与失败率，每格 6 分钟">
    <div v-for="row in rows" :key="row.id" class="grid h-8 grid-cols-[minmax(64px,0.75fr)_minmax(0,1fr)_minmax(0,1fr)] items-center gap-x-2">
      <DataTooltip placement="top" :content="row.name" class="min-w-0" content-class="min-w-24">
        <span class="block truncate font-medium text-foreground/75">{{ row.name }}</span>
        <span v-if="row.stale" class="sr-only">暂无新数据</span>
      </DataTooltip>
      <div v-for="metric in metrics" :key="metric" role="group" class="group/panel relative flex h-7 min-w-0 flex-col gap-1 text-left" :aria-label="`${row.name} ${metric === 'latency' ? '延迟' : (row.kind === 'icmp' ? '丢包率' : '失败率')} ${row.stale ? '暂无新数据' : row[metric]}`">
        <div class="relative flex items-center justify-between leading-none">
          <span class="shrink-0 text-muted-foreground">{{ metric === 'latency' ? '延迟' : (row.kind === 'icmp' ? '丢包' : '失败') }}</span>
          <div class="mx-1 flex-1 border-t-2 border-dotted border-gray-500/10" />
          <span class="shrink-0 font-medium tabular-nums text-foreground/85">{{ row.stale ? '—' : row[metric] }}</span>
        </div>
        <div class="grid h-2 grid-cols-10 items-end gap-px opacity-80 group-hover/panel:opacity-100">
          <DataTooltip
            v-for="(point, i) in row.points" :key="i" placement="top" class="h-full w-full" content-class="w-40 leading-relaxed"
            :content="row.stale ? `${row.name} · 暂无新数据` : sampleTooltip(row.name, point, metric)"
          >
            <span
              class="block h-full w-full rounded-[1px] bg-muted-foreground/15 transition-transform duration-150 group-hover/data-tooltip:scale-y-200 motion-reduce:transition-none"
              :style="!point.samples || row.stale ? undefined : { backgroundColor: metric === 'latency' ? tone(point.latency, point.loss < 100) : lossTone(point.loss), opacity: metric === 'latency' ? 0.9 : 0.86 }"
            />
          </DataTooltip>
        </div>
      </div>
    </div>
    <span v-if="!rows.length" class="text-muted-foreground">未配置探测</span>
  </div>
  <div v-else class="flex h-8 w-full flex-col justify-center gap-px overflow-hidden" aria-label="三网延迟概览，点击查看详情">
    <div v-for="row in rows" :key="row.id" class="grid h-[3px] shrink-0 grid-cols-10 gap-px" :title="`${row.name} · 延迟 ${row.latency} · 失败率 ${row.loss}`">
      <span v-for="(point, i) in row.points" :key="i" class="h-full bg-muted-foreground/15" :style="!point.samples || row.stale ? undefined : { backgroundColor: tone(point.latency, point.loss < 100) }" />
    </div>
    <span v-if="!rows.length" class="text-[11px] text-muted-foreground">未配置探测</span>
  </div>
</template>
