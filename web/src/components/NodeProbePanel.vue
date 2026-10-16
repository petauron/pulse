<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { defineAsyncComponent, ref } from 'vue'
import NodeProbeSummary from '@/components/NodeProbeSummary.vue'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useBackgroundSurface } from '@/composables/useBackgroundSurface'

defineProps<{ node: NodeData, compact?: boolean }>()
const open = ref(false)
const ProbeHistory = defineAsyncComponent(() => import('@/components/ProbeHistory.vue'))
const { pickSurfaceClass } = useBackgroundSurface()
</script>

<template>
  <div @click.stop>
    <div
      v-if="compact" role="button" tabindex="0" class="block w-full rounded-sm text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      :aria-label="`${node.name} 延迟 / 丢包详情`" @click="open = true" @keydown.enter.self.prevent="open = true" @keydown.space.self.prevent="open = true"
    >
      <NodeProbeSummary :probes="node.probes" compact :node-uuid="node.uuid" />
    </div>
    <NodeProbeSummary v-else :probes="node.probes" :node-uuid="node.uuid" :node-name="node.name" interactive @open="open = true" />
    <Dialog v-model:open="open">
      <DialogContent
        class="max-w-6xl gap-0 overflow-hidden border-emerald-600/10 p-0 shadow-[0_0_2rem] shadow-emerald-800/10 transition-all"
        :class="pickSurfaceClass('bg-background', 'bg-background/60')"
        @click.stop
      >
        <DialogHeader class="relative isolate flex h-13 flex-row items-center overflow-hidden px-4">
          <DialogTitle class="relative z-10 min-w-0 flex-1 truncate pr-8">{{ node.name }} 延迟 / 丢包</DialogTitle>
          <DialogDescription class="sr-only">TCP/HTTP 显示探测失败率，不等同于 ICMP 丢包。详情按所选时间范围统计。</DialogDescription>
          <div aria-hidden="true" class="pointer-events-none absolute inset-0 bg-slate-50 dark:bg-slate-900/50">
            <div class="absolute top-0 left-1/2 -ml-152 h-100 w-325 dark:mask-[linear-gradient(white,transparent)]">
              <div class="absolute inset-0 bg-linear-to-r from-emerald-500 to-lime-300 mask-[radial-gradient(farthest-side_at_top,white,transparent)] opacity-40 dark:from-emerald-500/30 dark:to-lime-300/30 dark:opacity-100">
                <svg class="absolute inset-x-0 inset-y-[-50%] h-[200%] w-full skew-y-[-18deg] fill-black/40 stroke-black/50 mix-blend-overlay dark:fill-white/2.5 dark:stroke-white/5">
                  <defs>
                    <pattern id="probe-panel-grid" width="72" height="56" patternUnits="userSpaceOnUse" x="-12" y="4">
                      <path d="M.5 56V.5H72" fill="none" />
                    </pattern>
                  </defs>
                  <rect width="100%" height="100%" stroke-width="0" fill="url(#probe-panel-grid)" />
                  <svg x="-12" y="4" class="overflow-visible">
                    <rect stroke-width="0" width="73" height="57" x="288" y="168" />
                    <rect stroke-width="0" width="73" height="57" x="144" y="56" />
                    <rect stroke-width="0" width="73" height="57" x="504" y="168" />
                    <rect stroke-width="0" width="73" height="57" x="720" y="336" />
                  </svg>
                </svg>
              </div>
            </div>
          </div>
        </DialogHeader>
        <div class="max-h-[calc(90dvh-4rem)] overflow-y-auto p-4 pt-0">
          <ProbeHistory v-if="open" :uuid="node.uuid" />
        </div>
      </DialogContent>
    </Dialog>
  </div>
</template>
