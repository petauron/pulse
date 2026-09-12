<script setup lang="ts">
import type { ManagedNode } from '@/utils/admin'
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useAdminAction } from '@/composables/useAdminAction'
import { nodeMetadata } from '@/utils/admin'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ nodes: ManagedNode[], refresh: () => Promise<void> }>()
const route = useRoute()
const router = useRouter()
const selected = computed(() => props.nodes.find(n => n.id === route.query.node) ?? props.nodes[0])
const draft = ref<ManagedNode | null>(null)
const expires = ref('')
const quotaGiB = ref(0)
const ttlHours = ref(24)
const secret = ref('')
const confirm = ref<'rotate' | 'revoke' | 'delete' | 'reset-traffic' | null>(null)
const { busy, error, success, run } = useAdminAction(props.refresh)
const actionLabels = { 'rotate': '轮换 Agent 凭据', 'revoke': '撤销 Agent 凭据', 'delete': '删除节点', 'reset-traffic': '重置本周期流量' }
const actionHints = {
  'rotate': '旧凭据会立即失效。请保存新凭据并更新对应 Agent，否则节点将停止上报。',
  'revoke': '对应 Agent 将无法继续上报。需要重新注册后才能恢复。',
  'delete': '将删除此节点及相关数据，无法从面板恢复。',
  'reset-traffic': '将本周期流量计数归零，不会重置主机的网络接口计数。',
}

watch(selected, (node) => {
  draft.value = node ? nodeMetadata(node) : null
  quotaGiB.value = (node?.traffic_limit_bytes ?? 0) / 1024 ** 3
  if (node?.expired_at_unix_ms) {
    const date = new Date(node.expired_at_unix_ms)
    expires.value = new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString().slice(0, 16)
  }
  else { expires.value = '' }
}, { immediate: true })

function choose(event: Event): void {
  secret.value = ''
  void router.replace({ query: { ...route.query, node: (event.target as HTMLSelectElement).value } })
}

async function save(): Promise<void> {
  if (!draft.value)
    return
  const node = { ...draft.value, expired_at_unix_ms: expires.value ? new Date(expires.value).getTime() : null, traffic_limit_bytes: Math.round(quotaGiB.value * 1024 ** 3) }
  await run(async () => {
    await getSharedApi().post(`admin/nodes/${encodeURIComponent(node.id)}`, node)
  })
}

async function enroll(): Promise<void> {
  secret.value = ''
  await run(async () => {
    const response = await getSharedApi().post<{ token: string }>('admin/enrollment', { ttl_seconds: ttlHours.value * 3600 })
    secret.value = response.token
  }, '已生成注册令牌')
}

async function execute(): Promise<void> {
  if (!confirm.value || !selected.value)
    return
  const action = confirm.value
  const id = selected.value.id
  secret.value = ''
  await run(async () => {
    if (action === 'rotate') {
      const response = await getSharedApi().post<{ agent_token: string }>(`admin/nodes/${encodeURIComponent(id)}/rotate`)
      secret.value = response.agent_token
    }
    else {
      await getSharedApi().post(`admin/nodes/${encodeURIComponent(id)}/${action}`)
    }
    confirm.value = null
  }, '操作已完成')
}
</script>

<template>
  <div class="space-y-4">
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p>
    <p v-if="success" role="status" class="text-sm text-success">
      {{ success }}
    </p>
    <CardX title="接入节点" content-class="space-y-4">
      <form class="flex flex-wrap items-end gap-3" @submit.prevent="enroll">
        <label class="grid gap-2 text-sm">注册令牌有效期（小时）<Input v-model="ttlHours" type="number" min="1" max="24" required class="max-w-48" :disabled="busy" /></label>
        <Button type="submit" class="min-h-11" :disabled="busy">
          生成一次性注册令牌
        </Button>
      </form>
      <p class="text-sm text-muted-foreground">
        令牌仅用于注册一个 Agent，请通过受保护的配置文件交给对应服务器。
      </p>
      <div v-if="secret" class="space-y-2 rounded-md border p-3">
        <label for="agent-secret" class="text-sm font-medium">新令牌（离开页面后清除）</label>
        <Input id="agent-secret" :model-value="secret" readonly autocomplete="off" class="font-mono" @focus="($event.target as HTMLInputElement).select()" />
        <Button variant="outline" type="button" @click="secret = ''">
          隐藏令牌
        </Button>
      </div>
    </CardX>
    <CardX title="节点管理" content-class="space-y-4">
      <label v-if="nodes.length" for="managed-node" class="grid gap-2 text-sm">选择节点
        <select id="managed-node" :value="selected?.id" class="h-11 rounded-md border border-input bg-background px-3 focus-visible:ring-2 focus-visible:ring-ring" :disabled="busy" @change="choose">
          <option v-for="node in nodes" :key="node.id" :value="node.id">{{ node.name }} · {{ node.id.slice(0, 8) }}{{ node.hidden ? ' · 已隐藏' : '' }}{{ node.disabled ? ' · 凭据已撤销' : '' }}</option>
        </select>
      </label>
      <p v-else class="text-sm text-muted-foreground">
        尚无节点。生成注册令牌并安装 Agent 后，节点会出现在这里。
      </p>
      <form v-if="draft" class="space-y-5" :aria-busy="busy" @submit.prevent="save">
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <legend class="mb-3 font-medium">
            显示与分组
          </legend>
          <label class="grid gap-2 text-sm">名称<Input v-model="draft.name" required maxlength="128" /></label>
          <label class="grid gap-2 text-sm">地区代码<Input v-model="draft.region" placeholder="例如 SG" maxlength="16" /></label>
          <label class="grid gap-2 text-sm">分组<Input v-model="draft.group" maxlength="128" /></label>
          <label class="grid gap-2 text-sm">排序权重<Input v-model="draft.weight" type="number" min="-2147483648" max="2147483647" step="1" required /></label>
          <label class="grid gap-2 text-sm">标签<Input v-model="draft.tags" placeholder="使用分号分隔" maxlength="512" /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.hidden" type="checkbox" class="size-4 accent-primary">从监控视图隐藏（管理列表保留）</label>
          <label class="grid gap-2 text-sm sm:col-span-2 lg:col-span-3">公开备注<textarea v-model="draft.public_remark" rows="3" maxlength="2048" class="rounded-md border border-input bg-background px-3 py-2 focus-visible:ring-2 focus-visible:ring-ring" /></label>
        </fieldset>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <legend class="mb-3 font-medium">
            费用与到期
          </legend>
          <label class="grid gap-2 text-sm">每周期价格<Input v-model="draft.price" type="number" min="0" max="1000000000" step="0.01" required /></label>
          <label class="grid gap-2 text-sm">币种（大写三字母）<Input v-model="draft.currency" placeholder="USD" pattern="[A-Z]{3}" minlength="3" maxlength="3" required /></label>
          <label class="grid gap-2 text-sm">计费周期（天）<Input v-model="draft.billing_cycle_days" type="number" min="0" max="36500" step="1" required /></label>
          <label class="grid gap-2 text-sm">到期时间（本地时间）<Input v-model="expires" type="datetime-local" /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.auto_renewal" type="checkbox" class="size-4 accent-primary">自动续费标记</label>
          <p class="text-sm text-muted-foreground sm:col-span-2 lg:col-span-3">
            这些信息用于资产记录和提醒，Pulse 不会执行支付或扣款。
          </p>
        </fieldset>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-3">
          <legend class="mb-3 font-medium">
            流量周期
          </legend>
          <label class="grid gap-2 text-sm">每周期额度（GiB；0 为不限）<Input v-model="quotaGiB" type="number" min="0" :max="Number.MAX_SAFE_INTEGER / 1024 ** 3" step="any" required /></label>
          <label class="grid gap-2 text-sm">流量计数方式<select v-model="draft.traffic_limit_type" class="h-11 rounded-md border border-input bg-background px-3 focus-visible:ring-2 focus-visible:ring-ring"><option value="sum">上传 + 下载</option><option value="max">上传、下载中较大值</option><option value="min">上传、下载中较小值</option><option value="up">仅上传</option><option value="down">仅下载</option></select></label>
          <label class="grid gap-2 text-sm">每月重置日（1–28）<Input v-model="draft.traffic_reset_day" type="number" min="1" max="28" step="1" required /></label>
        </fieldset>
        <div class="flex flex-wrap gap-2">
          <Button class="min-h-11" type="submit" :disabled="busy">
            {{ busy ? '正在处理…' : '保存节点' }}
          </Button><Button variant="outline" type="button" :disabled="selected?.hidden" @click="router.push(`/instance/${draft.id}`)">
            查看监控
          </Button>
        </div>
        <p v-if="selected?.hidden" class="text-sm text-muted-foreground">
          此节点已从监控页面和探测历史中隐藏。取消隐藏并保存后可查看。
        </p>
      </form>
      <div v-if="draft" class="flex flex-wrap gap-2 border-t pt-4">
        <Button v-for="(label, action) in actionLabels" :key="action" variant="outline" type="button" :disabled="busy" :class="action === 'delete' ? 'text-destructive' : ''" @click="confirm = action">
          {{ label }}
        </Button>
      </div>
    </CardX>
    <Dialog :open="confirm !== null" @update:open="value => { if (!value && !busy) confirm = null }">
      <DialogContent :show-close="!busy" @escape-key-down="event => { if (busy) event.preventDefault() }" @interact-outside="event => { if (busy) event.preventDefault() }">
        <DialogHeader><DialogTitle>{{ confirm ? actionLabels[confirm] : '' }}</DialogTitle><DialogDescription>{{ selected?.name }}：{{ confirm ? actionHints[confirm] : '' }}</DialogDescription></DialogHeader>
        <p v-if="error" role="alert" class="text-sm text-destructive">
          {{ error }}
        </p>
        <DialogFooter>
          <Button variant="outline" :disabled="busy" @click="confirm = null">
            取消
          </Button><Button variant="destructive" :disabled="busy" @click="execute">
            {{ busy ? '正在处理…' : '确认执行' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
