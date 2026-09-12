<script setup lang="ts">
import type { EChartsOption } from 'echarts'
import type { ProbeHistory } from '@/utils/admin'
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import VChart from 'vue-echarts'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { useAppStore } from '@/stores/app'
import { fetchProbeHistory } from '@/utils/admin'
import '@/utils/echarts'

const props = defineProps<{ uuid: string }>()
const app = useAppStore()
const hours = ref(24)
const data = shallowRef<ProbeHistory | null>(null)
const loading = ref(false)
const error = ref('')
let controller: AbortController | null = null
let generation = 0

async function reload(): Promise<void> {
  controller?.abort()
  const current = ++generation
  controller = new AbortController()
  loading.value = true
  error.value = ''
  try {
    const response = await fetchProbeHistory(props.uuid, hours.value, controller.signal)
    if (current === generation)
      data.value = response
  }
  catch (cause) {
    if (current === generation)
      error.value = cause instanceof Error ? cause.message : String(cause)
  }
  finally {
    if (current === generation)
      loading.value = false
  }
}

watch(() => [props.uuid, hours.value], () => {
  data.value = null
  void reload()
}, { immediate: true })
onBeforeUnmount(() => {
  ++generation
  controller?.abort()
})
const names = computed(() => new Map(data.value?.tasks.map(task => [task.id, task.name]) ?? []))
const recent = computed(() => [...(data.value?.records ?? [])].sort((a, b) => b.received_at_unix_ms - a.received_at_unix_ms).slice(0, 100))
const date = (value: number) => new Date(value).toLocaleString()
const latency = (value: number | null | undefined) => value == null ? '无成功样本' : `${value.toFixed(1)} ms`

const option = computed<EChartsOption>(() => {
  const style = getComputedStyle(document.documentElement)
  const tokens = ['--chart-1', '--chart-2', '--chart-3', '--chart-4', '--chart-5']
  const foreground = style.getPropertyValue('--foreground').trim()
  return {
    darkMode: app.isDark,
    animation: false,
    color: tokens.map(token => style.getPropertyValue(token).trim()),
    aria: { enabled: true },
    textStyle: { color: foreground },
    tooltip: { trigger: 'axis', renderMode: 'richText', confine: true },
    legend: { type: 'plain', bottom: 0, textStyle: { color: foreground } },
    grid: { left: 55, right: 20, top: 20, bottom: 65 },
    xAxis: { type: 'time', axisLabel: { color: foreground } },
    yAxis: { type: 'value', name: 'ms', min: 0, axisLabel: { color: foreground }, splitLine: { lineStyle: { color: style.getPropertyValue('--border').trim() } } },
    series: (data.value?.tasks ?? []).map(task => ({
      name: task.name,
      type: 'line',
      showSymbol: false,
      connectNulls: false,
      data: (data.value?.records ?? []).filter(record => record.task_id === task.id).sort((a, b) => a.received_at_unix_ms - b.received_at_unix_ms).map(record => [record.received_at_unix_ms, record.success ? record.latency_ms : null]),
    })),
  }
})
</script>

<template>
  <CardX title="连通性与延迟" content-class="space-y-4">
    <template #header-extra>
      <div class="flex flex-wrap gap-2">
        <label class="sr-only" :for="`probe-hours-${uuid}`">探测历史时间范围</label>
        <select :id="`probe-hours-${uuid}`" v-model.number="hours" class="h-9 rounded-md border border-input bg-background px-2 text-sm focus-visible:ring-2 focus-visible:ring-ring">
          <option :value="1">
            最近 1 小时
          </option><option :value="6">
            最近 6 小时
          </option><option :value="24">
            最近 24 小时
          </option><option :value="168">
            最近 7 天
          </option>
        </select>
        <Button variant="outline" :disabled="loading" @click="reload">
          {{ loading ? '加载中…' : '刷新' }}
        </Button>
      </div>
    </template>
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p>
    <p v-if="loading && !data" role="status" class="text-sm text-muted-foreground">
      正在读取探测记录…
    </p>
    <template v-if="data">
      <p v-if="!data.tasks.length" class="text-sm text-muted-foreground">
        此节点尚未配置探测任务。管理员可添加 ICMP、TCP 或 HTTP 探测。
      </p>
      <template v-else>
        <p v-if="data.records.length >= data.limit" class="text-sm text-muted-foreground">
          曲线仅展示最近 {{ data.limit }} 个样本；汇总按整个所选时间范围计算。
        </p>
        <div class="overflow-x-auto">
          <table class="w-full text-left text-sm">
            <caption class="sr-only">
              所选时间范围的延迟与丢包汇总
            </caption><thead>
              <tr class="border-b">
                <th class="p-2">
                  任务
                </th><th class="p-2">
                  样本
                </th><th class="p-2">
                  平均延迟
                </th><th class="p-2">
                  失败 / 丢包率
                </th>
              </tr>
            </thead><tbody>
              <tr v-for="task in data.tasks" :key="task.id" class="border-b">
                <th class="p-2 font-medium">
                  {{ task.name }}
                </th><td class="p-2">
                  {{ data.summary.find(s => s.task_id === task.id)?.samples ?? 0 }}
                </td><td class="p-2">
                  {{ latency(data.summary.find(s => s.task_id === task.id)?.avg_latency_ms) }}
                </td><td class="p-2">
                  {{ data.summary.find(s => s.task_id === task.id)?.samples ? `${data.summary.find(s => s.task_id === task.id)!.loss_percent.toFixed(1)}%` : '无样本' }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <VChart v-if="data.records.length" :key="app.resolvedThemeMode" :option="option" autoresize class="h-72 w-full" aria-label="探测延迟曲线，失败样本显示为断点，数值可在下方表格查看" />
        <p v-else class="text-sm text-muted-foreground">
          此时间范围暂无上报样本；请等待 Agent 执行任务后刷新。
        </p>
        <details v-if="recent.length" class="rounded-md border p-3">
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
  </CardX>
</template>
