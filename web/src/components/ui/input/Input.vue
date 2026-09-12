<script setup lang="ts" generic="T extends string | number">
import type { HTMLAttributes } from 'vue'
import { cn } from '@/lib/utils'

const props = defineProps<{
  defaultValue?: T
  type?: string
  class?: HTMLAttributes['class']
}>()

const modelValue = defineModel<T>({
  get: value => value ?? props.defaultValue,
})
</script>

<template>
  <input
    v-model="modelValue"
    :type="type"
    data-slot="input"
    :class="cn(
      'bg-transparent border-input focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:aria-invalid:border-destructive/50 h-9 rounded-md border px-3 py-1 text-base shadow-xs transition-colors file:h-7 file:text-sm file:font-medium focus-visible:ring-[3px] aria-invalid:ring-[3px] md:text-sm w-full min-w-0 outline-none file:inline-flex file:border-0 file:bg-transparent file:text-foreground placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30',
      props.class,
    )"
  >
</template>
