<script setup lang="ts">
import type { TagColor } from '@/utils/tagHelper'
import { computed } from 'vue'
import { Badge } from '@/components/ui/badge'
import { DataTooltip } from '@/components/ui/data-tooltip'

const props = defineProps<{ text: string, color: TagColor }>()

const toneClass = computed(() => {
  if (['green', 'grass', 'jade', 'lime', 'mint', 'teal'].includes(props.color))
    return 'border-emerald-500/30 bg-emerald-500/10 text-emerald-800 dark:text-emerald-200'
  if (['blue', 'cyan', 'indigo', 'iris', 'sky'].includes(props.color))
    return 'border-blue-500/30 bg-blue-500/10 text-blue-800 dark:text-blue-200'
  if (['plum', 'purple', 'violet'].includes(props.color))
    return 'border-violet-500/30 bg-violet-500/10 text-violet-800 dark:text-violet-200'
  if (['amber', 'bronze', 'brown', 'gold', 'orange', 'yellow'].includes(props.color))
    return 'border-amber-500/30 bg-amber-500/10 text-amber-900 dark:text-amber-200'
  if (['crimson', 'pink', 'red', 'ruby', 'tomato'].includes(props.color))
    return 'border-rose-500/30 bg-rose-500/10 text-rose-800 dark:text-rose-200'
  return 'border-slate-500/30 bg-slate-500/10 text-slate-800 dark:text-slate-200'
})
</script>

<template>
  <DataTooltip as="span" :content="text" placement="top" class="inline-flex max-w-full min-w-0" tabindex="0" :aria-label="text">
    <Badge variant="outline" class="max-w-full min-w-0 rounded-full px-2.5 py-0.5 text-xs font-semibold" :class="toneClass">
      <span class="min-w-0 truncate">{{ text }}</span>
    </Badge>
  </DataTooltip>
</template>
