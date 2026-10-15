<script setup lang="ts">
import type { ProbeComparison } from '@/utils/admin'
import { Icon } from '@iconify/vue'
import { computed, defineAsyncComponent, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useRouter } from 'vue-router'
import NodeProbeSummary from '@/components/NodeProbeSummary.vue'
import { Button } from '@/components/ui/button'
import { useAppStore } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { fetchProbeComparison } from '@/utils/admin'

const app = useAppStore()
const store = useNodesStore()
const router = useRouter()
const kind = ref<'icmp' | 'tcp' | 'http'>('tcp')
const modes = [{ kind: 'icmp', label: 'ICMP 延迟详情' }, { kind: 'tcp', label: 'TCP 连接质量' }, { kind: 'http', label: 'HTTP 可用性' }] as const
const ranges = [1, 6, 12, 24, 72, 168]
const availableRanges = computed(() => ranges.filter(range => range <= (app.publicSettings?.ping_record_preserve_time ?? 168)))
const hours = ref(1)
watch(availableRanges, (available) => {
  if (!available.includes(hours.value))
    hours.value = available.at(-1) ?? 1
})
const taskId = ref('')
const tasks = computed(() => [...new Map(store.nodes.flatMap(node => node.probes ?? []).filter(task => task.kind === kind.value).map(task => [task.id, task])).values()])
const task = computed(() => tasks.value.find(item => item.id === taskId.value) ?? tasks.value[0])
const selectedId = ref('')
const nodes = computed(() => store.nodes.filter(node => node.probes?.some(probe => probe.id === task.value?.id)))
const selected = computed(() => nodes.value.find(node => node.uuid === selectedId.value) ?? nodes.value[0])
const summary = shallowRef<ProbeComparison[]>([])
const loading = ref(false)
const error = ref('')
let controller: AbortController | null = null
let refreshTimer: ReturnType<typeof setTimeout> | undefined
let generation = 0

async function reload(): Promise<void> {
  clearTimeout(refreshTimer)
  controller?.abort()
  const current = ++generation
  const selectedTask = task.value
  if (!selectedTask) {
    summary.value = []
    loading.value = false
    error.value = ''
    return
  }
  controller = new AbortController()
  summary.value = []
  loading.value = true
  error.value = ''
  try {
    const rows = await fetchProbeComparison(selectedTask.id, hours.value, controller.signal)
    if (current === generation)
      summary.value = rows
  }
  catch (cause) {
    if (current === generation) {
      summary.value = []
      error.value = cause instanceof Error ? cause.message : '读取探测汇总失败'
    }
  }
  finally {
    if (current === generation) {
      loading.value = false
      refreshTimer = setTimeout(() => void reload(), 30000)
    }
  }
}

watch(() => [task.value?.id, hours.value], () => void reload(), { immediate: true })
onBeforeUnmount(() => {
  clearTimeout(refreshTimer)
  ++generation
  controller?.abort()
})
const summaryByNode = computed(() => new Map(summary.value.map(row => [row.node_id, row])))
const comparisons = computed(() => nodes.value.map((node) => {
  const row = summaryByNode.value.get(node.uuid)
  return { node, samples: row?.samples ?? 0, latency: row?.avg_latency_ms?.toFixed(1) ?? '—', loss: row?.loss_percent.toFixed(1) ?? '—' }
}))
const ProbeHistory = defineAsyncComponent(() => import('@/components/ProbeHistory.vue'))
</script>

<template>
  <section class="space-y-4 p-4">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-3">
        <Button variant="ghost" size="icon-sm" aria-label="返回首页" @click="router.push('/')">
          <Icon icon="lucide:arrow-left" width="17" height="17" />
        </Button>
        <div>
          <h1 class="text-xl font-semibold">
            网络质量
          </h1><p class="mt-0.5 text-xs text-muted-foreground">
            按同一线路目标比较节点，所选范围内的完整样本
          </p>
        </div>
      </div>
      <label v-if="nodes.length" class="flex items-center gap-2 text-sm">
        节点
        <select :value="selected?.uuid" class="max-w-full rounded-md border border-border bg-background p-2" @change="selectedId = ($event.target as HTMLSelectElement).value">
          <option v-for="node in nodes" :key="node.uuid" :value="node.uuid">{{ node.name }}</option>
        </select>
      </label>
    </div>
    <nav class="flex flex-wrap gap-1 rounded-md bg-muted/70 p-1" aria-label="网络质量分析视图">
      <button v-for="mode in modes" :key="mode.kind" type="button" class="h-9 flex-1 rounded px-3 text-sm font-medium" :aria-pressed="kind === mode.kind" :class="kind === mode.kind ? 'bg-background text-emerald-700 shadow-sm dark:text-emerald-400' : 'text-muted-foreground hover:text-foreground'" @click="kind = mode.kind">
        {{ mode.label }}
      </button>
    </nav>
    <div class="flex flex-wrap items-center justify-between gap-3">
      <label v-if="tasks.length" class="flex items-center gap-2 text-sm">线路目标<select :value="task?.id" class="rounded-md border border-border bg-background p-2" @change="taskId = ($event.target as HTMLSelectElement).value"><option v-for="item in tasks" :key="item.id" :value="item.id">{{ item.name }}</option></select></label>
      <div class="flex flex-wrap gap-1 rounded-md bg-muted/70 p-1" role="group" aria-label="统计范围">
        <button v-for="range in availableRanges" :key="range" type="button" class="rounded px-2 py-1 text-xs" :aria-pressed="hours === range" :class="hours === range ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'" @click="hours = range">
          {{ range < 24 ? `${range} 小时` : `${range / 24} 天` }}
        </button>
      </div>
    </div>
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p>
    <p v-if="loading && !summary.length" role="status" class="text-sm text-muted-foreground">
      正在读取探测汇总…
    </p>
    <div v-if="comparisons.length" class="overflow-x-auto rounded-lg bg-background/60 p-3">
      <table class="w-full text-left text-sm">
        <caption class="sr-only">
          所选线路目标与时间范围的统计
        </caption>
        <thead>
          <tr class="border-b">
            <th class="p-2">
              节点
            </th><th class="p-2">
              平均延迟
            </th><th class="p-2">
              {{ kind === 'icmp' ? '丢包率' : '失败率' }}
            </th><th class="p-2">
              样本
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in comparisons" :key="row.node.uuid" class="border-b last:border-0">
            <th class="p-2">
              <button class="text-left hover:text-emerald-600" @click="selectedId = row.node.uuid">
                {{ row.node.name }}
              </button>
            </th><td class="p-2 tabular-nums">
              {{ row.latency }} ms
            </td><td class="p-2 tabular-nums">
              {{ row.loss }}%
            </td><td class="p-2 tabular-nums">
              {{ row.samples }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <template v-if="selected">
      <div v-if="hours === 1" class="max-w-lg rounded-lg bg-background/60 p-4">
        <NodeProbeSummary :probes="selected.probes?.filter(probe => probe.id === task?.id)" />
      </div>
      <p class="text-xs text-muted-foreground">
        表格汇总按完整所选时间范围计算。失败率表示未成功的探测比例；TCP、HTTP 失败不等于 ICMP 丢包。
      </p>
      <ProbeHistory :key="selected.uuid" :uuid="selected.uuid" :kind="kind" :task-id="task?.id" :selected-hours="hours" />
    </template>
    <p v-else class="text-sm text-muted-foreground">
      当前类别暂无探测数据，可切换类别或由管理员添加相应任务。
    </p>
  </section>
</template>
