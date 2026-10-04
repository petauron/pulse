<script setup lang="ts">
import type { NodeProbe } from '@/utils/rpc'
import { computed } from 'vue'
import { DataTooltip } from '@/components/ui/data-tooltip'

const props = defineProps<{ probes?: NodeProbe[] }>()
const metrics = ['latency', 'loss'] as const
const rows = computed(() => (props.probes ?? []).map((task) => {
  const last = task.points.at(-1)
  const stale = !last || Date.now() - last.time > Math.max(90000, task.interval_seconds * 3000)
  const points = task.points.slice(-20)
  const successes = points.filter(p => p.success && p.latency !== null)
  return {
    ...task,
    stale,
    points,
    latency: !stale && successes.length ? `${Math.round(successes.reduce((sum, p) => sum + p.latency!, 0) / successes.length)} ms` : '—',
    loss: !stale && points.length ? `${(100 * points.filter(p => !p.success).length / points.length).toFixed(0)}%` : '—',
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
function sampleTooltip(name: string, point: NodeProbe['points'][number], metric: typeof metrics[number]): string {
  const result = metric === 'loss'
    ? (point.success ? '成功 · 0%' : '失败 · 100%')
    : !point.success ? '探测失败' : point.latency === null ? '无延迟数据' : `${Math.round(point.latency)} ms`
  return `${name} · ${new Date(point.time).toLocaleTimeString()} · ${result}`
}
</script>

<template>
  <div class="flex flex-col gap-y-2 text-[11px]" aria-label="最近最多 20 次探测的延迟与失败率">
    <div v-for="row in rows" :key="row.id" class="grid h-8 grid-cols-[minmax(64px,0.75fr)_minmax(0,1fr)_minmax(0,1fr)] items-center gap-x-2">
      <DataTooltip placement="top" :content="row.name" class="min-w-0" content-class="min-w-24">
        <span class="block truncate font-medium text-foreground/75">{{ row.name }}</span>
        <span v-if="row.stale" class="sr-only">暂无新数据</span>
      </DataTooltip>
      <div v-for="metric in metrics" :key="metric" role="group" class="group/panel relative flex h-7 min-w-0 flex-col gap-1 text-left" :aria-label="`${row.name} ${metric === 'latency' ? '延迟' : '失败率'} ${row.stale ? '暂无新数据' : row[metric]}`">
        <div class="relative flex items-center justify-between leading-none">
          <span class="shrink-0 text-muted-foreground">{{ metric === 'latency' ? '延迟' : '失败' }}</span>
          <div class="mx-1 flex-1 border-t-2 border-dotted border-gray-500/10" />
          <span class="shrink-0 font-medium tabular-nums text-foreground/85">{{ row.stale ? '—' : row[metric] }}</span>
        </div>
        <div class="grid h-2 grid-cols-20 items-end gap-px opacity-80 group-hover/panel:opacity-100">
          <span v-for="i in 20 - row.points.length" :key="`empty-${i}`" class="block h-full w-full rounded-[1px] bg-muted-foreground/10" />
          <DataTooltip
            v-for="(point, i) in row.points" :key="i" placement="top" class="h-full w-full" content-class="w-40 leading-relaxed"
            :content="row.stale ? `${row.name} · 暂无新数据` : sampleTooltip(row.name, point, metric)"
          >
            <span
              class="block h-full w-full rounded-[1px] bg-muted-foreground/15 transition-transform duration-150 group-hover/data-tooltip:scale-y-200 motion-reduce:transition-none"
              :style="row.stale ? undefined : { backgroundColor: metric === 'latency' ? tone(point.latency, point.success) : tone(0, point.success), opacity: metric === 'latency' ? 0.9 : 0.86 }"
            />
          </DataTooltip>
        </div>
      </div>
    </div>
    <span v-if="!rows.length" class="text-muted-foreground">未配置探测</span>
  </div>
</template>
