<script setup lang="ts">
import type { AlertRule, Incident, ManagedNode, NotificationChannel } from '@/utils/admin'
import { computed, ref } from 'vue'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useAdminAction } from '@/composables/useAdminAction'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ channels: NotificationChannel[], rules: AlertRule[], incidents: Incident[], failures: { happened_at_unix_ms: number, subject: string }[], nodes: ManagedNode[], refresh: () => Promise<void> }>()
const emptyChannel = (): NotificationChannel => ({ id: '', name: '', kind: 'webhook', url: '', enabled: false })
const emptyRule = (): AlertRule => ({ id: '', name: '', metric: 'offline', threshold: 0, duration_seconds: 120, cooldown_seconds: 900, node_ids: [], channel_ids: [], enabled: true })
const channel = ref(emptyChannel())
const rule = ref(emptyRule())
const removing = ref<{ kind: 'channels' | 'alert-rules', id: string, name: string } | null>(null)
const { busy, error, success, run } = useAdminAction(props.refresh)
const metricLabels = { offline: '节点离线', cpu: 'CPU 使用率', memory: '内存使用率', disk: '磁盘使用率', traffic: '流量额度使用率', expiry: '到期提醒' }
const thresholdLabel = computed(() => rule.value.metric === 'expiry' ? '提前提醒（天）' : '阈值（%）')
const date = (value: number) => new Date(value).toLocaleString()
const nodeName = (id: string) => props.nodes.find(node => node.id === id)?.name ?? id
const incidentLabels: Record<string, string> = { pending: '等待持续条件', firing: '告警中', resolved: '已恢复' }
const incidentStatus = (status: string) => incidentLabels[status] ?? status

async function saveChannel(): Promise<void> {
  await run(async () => {
    await getSharedApi().post('admin/channels', { ...channel.value, id: channel.value.id || undefined })
    channel.value = emptyChannel()
  }, '已保存通知渠道')
}

async function saveRule(): Promise<void> {
  await run(async () => {
    await getSharedApi().post('admin/alert-rules', { ...rule.value, id: rule.value.id || undefined })
    rule.value = emptyRule()
  }, '已保存告警规则')
}

async function remove(): Promise<void> {
  if (!removing.value)
    return
  await run(async () => {
    const item = removing.value!
    await getSharedApi().post(`admin/${item.kind}/${encodeURIComponent(item.id)}/delete`)
    if (item.id === channel.value.id)
      channel.value = emptyChannel()
    if (item.id === rule.value.id)
      rule.value = emptyRule()
    removing.value = null
  }, '已删除')
}
</script>

<template>
  <div class="space-y-4">
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p><p v-if="success" role="status" class="text-sm text-success">
      {{ success }}
    </p>
    <CardX title="Webhook 通知渠道" content-class="space-y-4">
      <p class="text-sm text-muted-foreground">
        渠道默认关闭。显式启用后向配置的 HTTPS 地址发送节点名称与 ID、规则、状态、时间及简短原因；不发送 IP、凭据或原始指标快照。请妥善保管 URL 中的令牌。
      </p>
      <ul class="divide-y">
        <li v-for="item in channels" :key="item.id" class="flex flex-wrap items-center justify-between gap-2 py-3">
          <p class="font-medium">
            {{ item.name }} <span class="text-sm text-muted-foreground">{{ item.enabled ? '启用' : '停用' }}</span>
          </p><div class="flex gap-2">
            <Button variant="outline" :disabled="busy" @click="channel = { ...item }">
              编辑
            </Button><Button variant="outline" :disabled="busy" @click="removing = { kind: 'channels', id: item.id, name: item.name }">
              删除
            </Button>
          </div>
        </li>
      </ul>
      <form class="space-y-4 border-t pt-4" :aria-busy="busy" @submit.prevent="saveChannel">
        <h3 class="font-medium">
          {{ channel.id ? '编辑渠道' : '新增渠道' }}
        </h3>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2">
          <label class="grid gap-2 text-sm">渠道名称<Input v-model="channel.name" required maxlength="128" /></label><label class="grid gap-2 text-sm">Webhook URL<Input v-model="channel.url" type="url" pattern="https://.*" autocomplete="off" required maxlength="2048" placeholder="https://" /></label><label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="channel.enabled" type="checkbox" class="size-4 accent-primary">启用渠道并允许发送上述告警信息</label>
        </fieldset>
        <div class="flex gap-2">
          <Button type="submit" class="min-h-11" :disabled="busy">
            保存渠道
          </Button><Button v-if="channel.id" type="button" variant="outline" :disabled="busy" @click="channel = emptyChannel()">
            取消编辑
          </Button>
        </div>
      </form>
    </CardX>
    <CardX title="告警规则" content-class="space-y-4">
      <p v-if="!rules.length" class="text-sm text-muted-foreground">
        尚无告警规则。选择告警条件即可启用站内告警，通知渠道可按需配置。
      </p>
      <ul class="divide-y">
        <li v-for="item in rules" :key="item.id" class="flex flex-wrap items-center justify-between gap-2 py-3">
          <div>
            <p class="font-medium">
              {{ item.name }} <span class="text-sm text-muted-foreground">{{ item.enabled ? '启用' : '停用' }}</span>
            </p><p class="text-sm text-muted-foreground">
              {{ metricLabels[item.metric] }} · 持续 {{ item.duration_seconds }} 秒 · 冷却 {{ item.cooldown_seconds }} 秒
            </p>
          </div><div class="flex gap-2">
            <Button variant="outline" :disabled="busy" @click="rule = { ...item, node_ids: [...item.node_ids], channel_ids: [...item.channel_ids] }">
              编辑
            </Button><Button variant="outline" :disabled="busy" @click="removing = { kind: 'alert-rules', id: item.id, name: item.name }">
              删除
            </Button>
          </div>
        </li>
      </ul>
      <form class="space-y-4 border-t pt-4" :aria-busy="busy" @submit.prevent="saveRule">
        <h3 class="font-medium">
          {{ rule.id ? '编辑规则' : '新增规则' }}
        </h3>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <label class="grid gap-2 text-sm">规则名称<Input v-model="rule.name" required maxlength="128" /></label>
          <label class="grid gap-2 text-sm">监测条件<select v-model="rule.metric" class="h-11 rounded-md border border-input bg-background px-3 focus-visible:ring-2 focus-visible:ring-ring"><option v-for="(label, metric) in metricLabels" :key="metric" :value="metric">{{ label }}</option></select></label>
          <label v-if="rule.metric !== 'offline'" class="grid gap-2 text-sm">{{ thresholdLabel }}<Input v-model="rule.threshold" type="number" min="0" :max="rule.metric === 'expiry' ? 3650 : 100" step="any" required /></label>
          <label class="grid gap-2 text-sm">持续时间（秒）<Input v-model="rule.duration_seconds" type="number" min="0" max="86400" required /></label>
          <label class="grid gap-2 text-sm">通知冷却（秒）<Input v-model="rule.cooldown_seconds" type="number" min="60" max="604800" required /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="rule.enabled" type="checkbox" class="size-4 accent-primary">启用规则</label>
        </fieldset>
        <div class="grid gap-4 sm:grid-cols-2">
          <fieldset :disabled="busy" class="rounded-md border p-3">
            <legend class="px-1 text-sm font-medium">
              监测节点
            </legend><p class="mb-2 text-xs text-muted-foreground">
              不选择时应用于所有活动节点，包括之后注册的节点。
            </p><div class="max-h-64 space-y-2 overflow-auto">
              <label v-for="node in nodes.filter(item => !item.disabled)" :key="node.id" class="flex min-h-9 items-center gap-2 text-sm"><input v-model="rule.node_ids" type="checkbox" :value="node.id" class="size-4 accent-primary">{{ node.name }}</label>
            </div>
          </fieldset>
          <fieldset :disabled="busy" class="rounded-md border p-3">
            <legend class="px-1 text-sm font-medium">
              通知渠道（可选）
            </legend><p class="mb-2 text-xs text-muted-foreground">
              不选择渠道时，仅记录站内告警事件。
            </p><div class="max-h-64 space-y-2 overflow-auto">
              <label v-for="item in channels" :key="item.id" class="flex min-h-9 items-center gap-2 text-sm"><input v-model="rule.channel_ids" type="checkbox" :value="item.id" class="size-4 accent-primary">{{ item.name }}{{ item.enabled ? '' : '（已停用）' }}</label>
            </div>
          </fieldset>
        </div>
        <div class="flex gap-2">
          <Button type="submit" class="min-h-11" :disabled="busy">
            保存规则
          </Button><Button v-if="rule.id" type="button" variant="outline" :disabled="busy" @click="rule = emptyRule()">
            取消编辑
          </Button>
        </div>
      </form>
    </CardX>
    <CardX title="告警事件">
      <p v-if="!incidents.length" class="text-sm text-muted-foreground">
        尚无告警事件。
      </p>
      <div v-else class="max-h-[32rem] overflow-auto">
        <table class="w-full text-left text-sm">
          <caption class="sr-only">
            最近告警事件
          </caption><thead>
            <tr class="border-b">
              <th class="p-2">
                节点
              </th><th class="p-2">
                状态
              </th><th class="p-2">
                说明
              </th><th class="p-2">
                更新时间
              </th>
            </tr>
          </thead><tbody>
            <tr v-for="incident in incidents" :key="incident.id" class="border-b">
              <th class="p-2 font-medium">
                {{ nodeName(incident.node_id) }}
              </th><td class="p-2">
                {{ incidentStatus(incident.status) }}
              </td><td class="p-2">
                {{ incident.message }}
              </td><td class="whitespace-nowrap p-2">
                {{ date(incident.updated_at_unix_ms) }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </CardX>
    <CardX v-if="failures.length" title="最近通知投递失败">
      <ul class="max-h-64 space-y-2 overflow-auto text-sm">
        <li v-for="(failure, index) in failures" :key="index">
          <span class="text-muted-foreground">{{ date(failure.happened_at_unix_ms) }}</span> · {{ failure.subject }}
        </li>
      </ul>
    </CardX>
    <Dialog :open="removing !== null" @update:open="value => { if (!value && !busy) removing = null }">
      <DialogContent :show-close="!busy" @escape-key-down="event => { if (busy) event.preventDefault() }" @interact-outside="event => { if (busy) event.preventDefault() }">
        <DialogHeader><DialogTitle>{{ removing?.kind === 'channels' ? '删除通知渠道' : '删除告警规则' }}</DialogTitle><DialogDescription>确认删除“{{ removing?.name }}”？此操作会影响后续告警通知。</DialogDescription></DialogHeader><p v-if="error" role="alert" class="text-sm text-destructive">
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
