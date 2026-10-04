<script setup lang="ts">
import type { NodeProbe } from '@/utils/rpc'
import { computed } from 'vue'

const props = defineProps<{ probes?: NodeProbe[] }>()
const rows = computed(() => (props.probes ?? []).map((task) => {
  const last = task.points.at(-1)
  const stale = !last || Date.now() - last.time > Math.max(90000, task.interval_seconds * 3000)
  const successes = task.points.filter(p => p.success && p.latency !== null)
  return {
    ...task,
    stale,
    latency: !stale && successes.length ? `${Math.round(successes.reduce((sum, p) => sum + p.latency!, 0) / successes.length)} ms` : '—',
    loss: !stale && task.points.length ? `${(100 * task.points.filter(p => !p.success).length / task.points.length).toFixed(0)}%` : '—',
  }
}))
function tone(latency: number | null, success: boolean): string {
  if (!success)
    return 'bg-red-500'
  if (latency === null)
    return 'bg-muted'
  if (latency <= 100)
    return 'bg-emerald-500'
  if (latency <= 160)
    return 'bg-sky-500'
  if (latency <= 200)
    return 'bg-amber-500'
  return 'bg-orange-500'
}
</script>

<template>
  <div class="space-y-2 text-[11px]" aria-label="最近最多 20 次探测的延迟与失败率">
    <div v-for="row in rows" :key="row.id" class="grid grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_auto] items-center gap-2" :title="`${row.name} · 最近 ${row.points.length} 次；红色表示失败`">
      <span class="truncate text-muted-foreground">{{ row.name }}</span>
      <div class="min-w-0">
        <div class="flex justify-between gap-1 tabular-nums">
          <span>{{ row.stale ? '暂无新数据' : row.latency }}</span>
        </div>
        <div class="mt-1 flex h-1.5 gap-px" aria-hidden="true">
          <span v-for="i in 20 - row.points.length" :key="`empty-${i}`" class="flex-1 rounded-sm bg-muted" />
          <span v-for="(point, i) in row.points" :key="i" class="flex-1 rounded-sm" :class="row.stale ? 'bg-muted' : tone(point.latency, point.success)" />
        </div>
      </div>
      <span class="text-right tabular-nums text-muted-foreground" :aria-label="`失败率 ${row.loss}`">{{ row.loss }}</span>
    </div>
    <span v-if="!rows.length" class="text-muted-foreground">未配置探测</span>
  </div>
</template>
