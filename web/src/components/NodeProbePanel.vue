<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { defineAsyncComponent, ref } from 'vue'
import NodeProbeSummary from '@/components/NodeProbeSummary.vue'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'

defineProps<{ node: NodeData, compact?: boolean }>()
const open = ref(false)
const ProbeHistory = defineAsyncComponent(() => import('@/components/ProbeHistory.vue'))
</script>

<template>
  <div @click.stop>
    <div
      role="button" tabindex="0" class="block w-full rounded-sm text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      :aria-label="`${node.name} 延迟 / 失败率详情`" @click="open = true" @keydown.enter.self.prevent="open = true" @keydown.space.self.prevent="open = true"
    >
      <NodeProbeSummary :probes="node.probes" :compact="compact" />
    </div>
    <Dialog v-model:open="open">
      <DialogContent class="max-h-[85dvh] overflow-y-auto sm:max-w-4xl" @click.stop>
        <DialogHeader>
          <DialogTitle>{{ node.name }} · 网络质量</DialogTitle>
          <DialogDescription>首页显示最近 1 小时统计，每格 6 分钟；详情按所选时间范围统计。失败率表示探测未成功的比例。</DialogDescription>
        </DialogHeader>
        <ProbeHistory v-if="open" :uuid="node.uuid" />
      </DialogContent>
    </Dialog>
  </div>
</template>
