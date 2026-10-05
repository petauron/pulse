<script setup lang="ts">
import { Icon } from '@iconify/vue'
import { computed, defineAsyncComponent, ref } from 'vue'
import { useRouter } from 'vue-router'
import NodeProbeSummary from '@/components/NodeProbeSummary.vue'
import { Button } from '@/components/ui/button'
import { useNodesStore } from '@/stores/nodes'

const store = useNodesStore()
const router = useRouter()
const kind = ref<'icmp' | 'tcp' | 'http'>('icmp')
const modes = [{ kind: 'icmp', label: 'ICMP 延迟详情' }, { kind: 'tcp', label: 'TCP 连接质量' }, { kind: 'http', label: 'HTTP 可用性' }] as const
const taskId = ref('')
const tasks = computed(() => [...new Map(store.nodes.flatMap(node => node.probes ?? []).filter(task => task.kind === kind.value).map(task => [task.id, task])).values()])
const task = computed(() => tasks.value.find(item => item.id === taskId.value) ?? tasks.value[0])
const selectedId = ref('')
const nodes = computed(() => store.nodes.filter(node => node.probes?.some(probe => probe.id === task.value?.id)))
const selected = computed(() => nodes.value.find(node => node.uuid === selectedId.value) ?? nodes.value[0])
const comparisons = computed(() => nodes.value.map((node) => {
  const probe = node.probes?.find(item => item.id === task.value?.id)
  const points = probe?.points ?? []
  const samples = points.reduce((sum, point) => sum + point.samples, 0)
  const successful = points.reduce((sum, point) => sum + point.successful_samples, 0)
  return { node, samples, latency: successful ? (points.reduce((sum, point) => sum + (point.latency ?? 0) * point.successful_samples, 0) / successful).toFixed(1) : '—', loss: samples ? (points.reduce((sum, point) => sum + point.loss * point.samples, 0) / samples).toFixed(1) : '—' }
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
            按同一线路目标比较节点，最近 1 小时
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
    <label v-if="tasks.length" class="flex items-center gap-2 text-sm">线路目标<select :value="task?.id" class="rounded-md border border-border bg-background p-2" @change="taskId = ($event.target as HTMLSelectElement).value"><option v-for="item in tasks" :key="item.id" :value="item.id">{{ item.name }}</option></select></label>
    <div v-if="comparisons.length" class="overflow-x-auto rounded-lg bg-background/60 p-3">
      <table class="w-full text-left text-sm">
        <caption class="sr-only">
          所选线路目标最近一小时统计
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
      <div class="max-w-lg rounded-lg bg-background/60 p-4">
        <NodeProbeSummary :probes="selected.probes?.filter(probe => probe.id === task?.id)" />
        <p class="mt-3 text-xs text-muted-foreground">
          最近 1 小时统计，每格 6 分钟。失败率表示未成功的探测比例；TCP、HTTP 失败不等于 ICMP 丢包。
        </p>
      </div>
      <ProbeHistory :key="selected.uuid" :uuid="selected.uuid" :kind="kind" :task-id="task?.id" />
    </template>
    <p v-else class="text-sm text-muted-foreground">
      当前类别暂无探测数据，可切换类别或由管理员添加相应任务。
    </p>
  </section>
</template>
