<script setup lang="ts">
import type { ManagedNode, ProbeTask } from '@/utils/admin'
import { computed, ref, watch } from 'vue'
import ProbeHistory from '@/components/ProbeHistory.vue'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useAdminAction } from '@/composables/useAdminAction'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ probes: ProbeTask[], nodes: ManagedNode[], refresh: () => Promise<void> }>()
const blank = (): ProbeTask => ({ id: '', name: '', kind: 'icmp', target: '', interval_seconds: 60, timeout_seconds: 5, enabled: true, node_ids: [] })
const draft = ref(blank())
const removing = ref<ProbeTask | null>(null)
const visibleNodes = computed(() => props.nodes.filter(node => !node.hidden && !node.disabled))
const resultNode = ref(visibleNodes.value[0]?.id ?? '')
watch(visibleNodes, (nodes) => {
  if (!nodes.some(node => node.id === resultNode.value))
    resultNode.value = nodes[0]?.id ?? ''
})
const { busy, error, success, run } = useAdminAction(props.refresh)

async function save(): Promise<void> {
  await run(async () => {
    await getSharedApi().post('admin/probes', { ...draft.value, id: draft.value.id || undefined })
    draft.value = blank()
  })
}

async function remove(): Promise<void> {
  if (!removing.value)
    return
  await run(async () => {
    await getSharedApi().post(`admin/probes/${encodeURIComponent(removing.value!.id)}/delete`)
    if (draft.value.id === removing.value!.id)
      draft.value = blank()
    removing.value = null
  }, '已删除探测任务')
}
</script>

<template>
  <div class="space-y-4">
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p><p v-if="success" role="status" class="text-sm text-success">
      {{ success }}
    </p>
    <CardX title="探测任务" content-class="space-y-4">
      <p v-if="!probes.length" class="text-sm text-muted-foreground">
        尚无探测任务。Agent 会按配置主动探测并上报结果。
      </p>
      <ul class="divide-y">
        <li v-for="task in probes" :key="task.id" class="flex flex-wrap items-center justify-between gap-3 py-3">
          <div class="min-w-0">
            <p class="font-medium">
              {{ task.name }} <span class="text-sm text-muted-foreground">{{ task.enabled ? '启用' : '停用' }}</span>
            </p><p class="break-all text-sm text-muted-foreground">
              {{ task.kind.toUpperCase() }} · {{ task.target }} · 每 {{ task.interval_seconds }} 秒
            </p>
          </div><div class="flex gap-2">
            <Button variant="outline" :disabled="busy" @click="draft = { ...task, node_ids: [...task.node_ids] }">
              编辑
            </Button><Button variant="outline" :disabled="busy" @click="removing = task">
              删除
            </Button>
          </div>
        </li>
      </ul>
    </CardX>
    <CardX :title="draft.id ? '编辑探测任务' : '新增探测任务'" content-class="space-y-4">
      <form class="space-y-4" :aria-busy="busy" @submit.prevent="save">
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <label class="grid gap-2 text-sm">名称<Input v-model="draft.name" required maxlength="128" /></label>
          <label class="grid gap-2 text-sm">类型<select v-model="draft.kind" class="h-11 rounded-md border border-input bg-background px-3 focus-visible:ring-2 focus-visible:ring-ring"><option value="icmp">ICMP</option><option value="tcp">TCP</option><option value="http">HTTP / HTTPS</option></select></label>
          <label class="grid gap-2 text-sm">目标<Input v-model="draft.target" :placeholder="draft.kind === 'http' ? 'https://example.com' : draft.kind === 'tcp' ? 'example.com:443' : 'example.com'" maxlength="2048" required /></label>
          <label class="grid gap-2 text-sm">间隔（秒）<Input v-model="draft.interval_seconds" type="number" min="5" max="3600" required /></label>
          <label class="grid gap-2 text-sm">超时（秒）<Input v-model="draft.timeout_seconds" type="number" min="1" :max="Math.min(30, draft.interval_seconds)" required /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.enabled" type="checkbox" class="size-4 accent-primary">启用任务</label>
        </fieldset>
        <fieldset :disabled="busy" class="space-y-2">
          <legend class="mb-2 text-sm font-medium">
            执行节点
          </legend><div class="grid max-h-64 gap-2 overflow-auto rounded-md border p-3 sm:grid-cols-2 lg:grid-cols-3">
            <label v-for="node in nodes.filter(item => !item.disabled)" :key="node.id" class="flex min-h-9 items-center gap-2 text-sm"><input v-model="draft.node_ids" type="checkbox" :value="node.id" class="size-4 accent-primary">{{ node.name }}</label>
          </div><p class="text-xs text-muted-foreground">
            不选择时应用于所有活动节点，包括之后注册的节点。
          </p>
        </fieldset>
        <div class="flex gap-2">
          <Button type="submit" class="min-h-11" :disabled="busy">
            {{ busy ? '正在保存…' : '保存任务' }}
          </Button><Button v-if="draft.id" type="button" variant="outline" :disabled="busy" @click="draft = blank()">
            取消编辑
          </Button>
        </div>
      </form>
    </CardX>
    <CardX v-if="visibleNodes.length" title="探测结果">
      <label for="probe-result-node" class="grid gap-2 text-sm">选择节点<select id="probe-result-node" v-model="resultNode" class="h-11 rounded-md border border-input bg-background px-3 focus-visible:ring-2 focus-visible:ring-ring"><option v-for="node in visibleNodes" :key="node.id" :value="node.id">{{ node.name }}</option></select></label>
    </CardX>
    <ProbeHistory v-if="resultNode" :uuid="resultNode" />
    <Dialog :open="removing !== null" @update:open="value => { if (!value && !busy) removing = null }">
      <DialogContent :show-close="!busy" @interact-outside="event => { if (busy) event.preventDefault() }" @escape-key-down="event => { if (busy) event.preventDefault() }">
        <DialogHeader><DialogTitle>删除探测任务</DialogTitle><DialogDescription>确认删除“{{ removing?.name }}”？Agent 将停止执行此任务。</DialogDescription></DialogHeader><p v-if="error" role="alert" class="text-sm text-destructive">
          {{ error }}
        </p><DialogFooter>
          <Button variant="outline" :disabled="busy" @click="removing = null">
            取消
          </Button><Button variant="destructive" :disabled="busy" @click="remove">
            确认删除
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
