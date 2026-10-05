<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import type { CurrencyCode } from '@/utils/financeHelper'
import { Icon } from '@iconify/vue'
import { useNow } from '@vueuse/core'
import { computed, nextTick, ref, useId, watch } from 'vue'
import NodeEarthGlobe from '@/components/NodeEarthGlobe.vue'
import { CardX } from '@/components/ui/card-x'
import { DataTooltip } from '@/components/ui/data-tooltip'
import { useBackgroundSurface } from '@/composables/useBackgroundSurface'
import { useExchangeRates } from '@/composables/useExchangeRates'
import { useAppStore } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { CURRENCY_SYMBOLS, FINANCE_CURRENCIES, summarizeNodeValue } from '@/utils/financeHelper'
import { formatBytesPerSecondSplit, formatBytesSplit } from '@/utils/helper'
import { summarizeNodeCapacity } from '@/utils/nodeSummary'

const props = defineProps<{
  nodes?: NodeData[]
  globeNodes?: NodeData[]
  transitionKey?: string
}>()

const appStore = useAppStore()
const { pickSurfaceClass } = useBackgroundSurface()
const nodesStore = useNodesStore()
const summaryNodes = computed(() => props.nodes ?? nodesStore.nodes)
const summaryTransitionKey = computed(() => props.transitionKey ?? 'all')
const metricSwitchTransitionProps = computed(() => ({
  ...(appStore.disablePageAnimation
    ? { css: false }
    : { name: 'metric-switch', mode: 'out-in' as const }),
}))

const openFinanceCard = ref(false)
const financeId = useId()
const closeFinanceButton = ref<HTMLButtonElement | null>(null)

async function toggleFinanceCard(): Promise<void> {
  if (openFinanceCard.value) {
    closeFinanceCard()
    return
  }
  openFinanceCard.value = true
  await nextTick()
  closeFinanceButton.value?.focus()
}

function closeFinanceCard(): void {
  openFinanceCard.value = false
  document.getElementById(`${financeId}-trigger`)?.focus()
}

function onFinanceFocusOut(event: FocusEvent): void {
  if (event.currentTarget instanceof HTMLElement
    && !event.currentTarget.contains(event.relatedTarget as Node | null)) {
    openFinanceCard.value = false
  }
}

function getMetricSwitchStyle(index: number): Record<string, string> {
  return {
    '--metric-switch-delay': `${index * 35}ms`,
  }
}

const totalSpeed = computed(() => {
  const onlineNodes = summaryNodes.value.filter(node => node.online)
  const up = onlineNodes.reduce((sum, node) => sum + (node.net_out || 0), 0)
  const down = onlineNodes.reduce((sum, node) => sum + (node.net_in || 0), 0)
  return { up, down }
})

const totalTraffic = computed(() => {
  const up = summaryNodes.value.reduce((sum, node) => sum + (node.net_total_up || 0), 0)
  const down = summaryNodes.value.reduce((sum, node) => sum + (node.net_total_down || 0), 0)
  return { up, down }
})

const formattedTrafficUp = computed(() => formatBytesSplit(totalTraffic.value.up, appStore.byteDecimals))
const formattedTrafficDown = computed(() => formatBytesSplit(totalTraffic.value.down, appStore.byteDecimals))
const totalTrafficTooltip = computed(() => formatBytesSplit(totalTraffic.value.up + totalTraffic.value.down, appStore.byteDecimals))

const formattedSpeedUp = computed(() => formatBytesPerSecondSplit(totalSpeed.value.up, appStore.byteDecimals))
const formattedSpeedDown = computed(() => formatBytesPerSecondSplit(totalSpeed.value.down, appStore.byteDecimals))

// 离线节点保留最后一次状态；当前用量只汇总在线节点，静态容量按全量统计。
const capacitySummary = computed(() => summarizeNodeCapacity(summaryNodes.value))
const totalMemory = computed(() => capacitySummary.value.memory)
const totalDisk = computed(() => capacitySummary.value.disk)

const formattedMemoryUsed = computed(() => formatBytesSplit(totalMemory.value.used, appStore.byteDecimals))
const formattedMemoryTotal = computed(() => formatBytesSplit(totalMemory.value.total, appStore.byteDecimals))
const formattedDiskUsed = computed(() => formatBytesSplit(totalDisk.value.used, appStore.byteDecimals))
const formattedDiskTotal = computed(() => formatBytesSplit(totalDisk.value.total, appStore.byteDecimals))

const financeCurrency = ref<CurrencyCode>('CNY')
const { rates, date: rateDate, live: dailyRates } = useExchangeRates()
watch(rates, (value) => {
  if (!value[financeCurrency.value])
    financeCurrency.value = 'CNY'
})
const now = useNow({ interval: 60000 })
const finance = computed(() => summarizeNodeValue(summaryNodes.value, now.value.getTime(), rates.value))
const formatValue = (value: number) => (value * (rates.value[financeCurrency.value] ?? 1)).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
const formattedRemainingValue = computed(() => ({
  value: `${dailyRates.value ? '' : '≈'}${CURRENCY_SYMBOLS[financeCurrency.value]}${formatValue(finance.value.remaining)}`,
  unit: financeCurrency.value,
}))
const financeSummaryItems = computed(() => [
  { label: '总价值', value: formatValue(finance.value.total), symbol: CURRENCY_SYMBOLS[financeCurrency.value] },
  { label: '月均支出', value: formatValue(finance.value.monthly), symbol: CURRENCY_SYMBOLS[financeCurrency.value] },
  { label: '剩余价值', value: formatValue(finance.value.remaining), symbol: CURRENCY_SYMBOLS[financeCurrency.value] },
])
const exchangeRateRows = computed(() => FINANCE_CURRENCIES.filter(currency => rates.value[currency]).map(currency => ({
  currency,
  rate: ((rates.value[currency] ?? 0) / (rates.value[financeCurrency.value] ?? 1)).toFixed(6),
  symbol: CURRENCY_SYMBOLS[currency],
})))
const showEarth = computed(() => appStore.earthViewMode === 'earth' || appStore.earthViewMode === 'earth-stop')
const showVisualPanel = computed(() => showEarth.value)
const wrapperClass = computed(() => showVisualPanel.value
  ? 'p-4 grid grid-cols-12 grid-rows-1 gap-2 h-auto md:h-58'
  : 'p-4 grid grid-cols-1 gap-2 h-auto')
const cardGridClass = computed(() => showVisualPanel.value
  ? 'h-42 -mt-42 md:mt-0 col-span-12 row-start-3 z-9 md:h-auto md:col-span-6 md:row-start-1 grid grid-cols-12 grid-rows-2 gap-2'
  : 'col-span-1 grid grid-cols-3 md:grid-cols-6 gap-2')
</script>

<template>
  <div :class="wrapperClass">
    <NodeEarthGlobe v-if="showEarth" :nodes="globeNodes" class="col-span-12 col-start-1 md:col-span-6 md:col-start-7" />

    <div :class="cardGridClass">
      <CardX
        hoverable
        class="group h-full border-none rounded-md transition-all"
        :class="[
          pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs'),
          showVisualPanel ? 'col-span-4 row-span-1 col-start-1 row-start-1' : 'col-span-1 row-start-1 col-start-1 min-h-18 md:min-h-24 md:row-start-1 md:col-start-1',
        ]"
        content-class="h-full !p-3"
      >
        <div class="flex h-full flex-col justify-between gap-1">
          <div class="flex items-start justify-between">
            <span class="text-xs font-medium tracking-wider text-muted-foreground">内存用量</span>
            <Icon
              icon="icon-park-outline:memory" :width="20" :height="20"
              class="text-slate-500/20 group-hover:text-slate-500 transition-colors"
            />
          </div>
          <Transition v-bind="metricSwitchTransitionProps">
            <div
              :key="`memory-${summaryTransitionKey}`" class="flex items-baseline gap-1 min-w-0"
              :style="getMetricSwitchStyle(0)"
            >
              <span class="text-md md:text-2xl font-bold leading-none tracking-tight">
                {{ formattedMemoryUsed.value }}
              </span>
              <span class="text-[11px] md:text-xs font-medium text-muted-foreground truncate">
                {{ formattedMemoryUsed.unit }} / {{ formattedMemoryTotal.value }} {{ formattedMemoryTotal.unit }}
              </span>
            </div>
          </Transition>
        </div>
      </CardX>
      <CardX
        hoverable
        class="group h-full border-none rounded-md transition-all"
        :class="[
          pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs'),
          showVisualPanel ? 'col-span-4 row-span-1 col-start-1 row-start-2' : 'col-span-1 row-start-2 col-start-1 min-h-18 md:min-h-24 md:row-start-1 md:col-start-2',
        ]"
        content-class="h-full !p-3"
      >
        <div class="flex h-full flex-col justify-between gap-1">
          <div class="flex items-start justify-between">
            <span class="text-xs font-medium tracking-wider text-muted-foreground">硬盘用量</span>
            <Icon
              icon="tabler:server-2" :width="20" :height="20"
              class="text-slate-500/20 group-hover:text-slate-500 transition-colors"
            />
          </div>
          <Transition v-bind="metricSwitchTransitionProps">
            <div
              :key="`disk-${summaryTransitionKey}`" class="flex items-baseline gap-1 min-w-0"
              :style="getMetricSwitchStyle(1)"
            >
              <span class="text-md md:text-2xl font-bold leading-none tracking-tight">{{ formattedDiskUsed.value
              }}</span>
              <span class="text-[11px] md:text-xs font-medium text-muted-foreground truncate">
                {{ formattedDiskUsed.unit }} / {{ formattedDiskTotal.value }} {{ formattedDiskTotal.unit }}
              </span>
            </div>
          </Transition>
        </div>
      </CardX>
      <div
        class="relative w-full h-full"
        :class="showVisualPanel ? 'col-span-4 row-span-1 col-start-5 row-start-1' : 'col-span-1 row-start-1 col-start-2 min-h-18 md:min-h-24 md:row-start-1 md:col-start-3'"
        @focusout="onFinanceFocusOut"
        @keydown.esc.stop.prevent="closeFinanceCard"
      >
        <CardX
          :id="`${financeId}-trigger`"
          role="button"
          tabindex="0"
          aria-label="查看剩余价值详情"
          :aria-controls="`${financeId}-panel`"
          :aria-expanded="openFinanceCard"
          hoverable
          class="group h-full border-none rounded-md transition-all cursor-pointer focus-visible:outline-2 focus-visible:outline-ring"
          :class="pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs')"
          content-class="h-full !p-3"
          @click="toggleFinanceCard"
          @keydown.enter.prevent="toggleFinanceCard"
          @keydown.space.prevent="toggleFinanceCard"
        >
          <div class="flex h-full flex-col justify-between gap-1">
            <div class="flex items-start justify-between">
              <span class="text-xs font-medium tracking-wider text-muted-foreground">剩余价值</span>
              <Icon
                icon="tabler:server-2" :width="20" :height="20"
                class="text-slate-500/20 group-hover:text-slate-500 transition-colors"
              />
            </div>
            <Transition v-bind="metricSwitchTransitionProps">
              <div
                :key="`remaining-value-${summaryTransitionKey}`" class="flex items-baseline gap-1 min-w-0"
                :style="getMetricSwitchStyle(2)"
              >
                <span class="text-md md:text-2xl font-bold leading-none tracking-tight">
                  {{ formattedRemainingValue.value }}
                </span>
                <span class="block truncate text-[11px] md:text-xs font-medium text-muted-foreground">
                  {{ formattedRemainingValue.unit }}
                </span>
              </div>
            </Transition>
          </div>
        </CardX>
        <CardX
          :id="`${financeId}-panel`"
          role="region"
          aria-label="剩余价值详情"
          :aria-hidden="!openFinanceCard"
          :inert="!openFinanceCard"
          hoverable
          class="absolute top-0 left-1/2 z-20 min-h-42 w-[260%] max-w-88 -translate-x-[50%] -translate-y-[25%] rounded-md border-none shadow-[0_0_20px,0_0_0_1px] shadow-emerald-600/10 transition-all"
          :class="[
            pickSurfaceClass('bg-background', 'bg-background/50 backdrop-blur-lg'),
            openFinanceCard ? 'opacity-100 scale-100  -translate-y-[5%]' : 'opacity-0 pointer-events-none scale-50',
          ]"
          content-class="h-full !p-4"
        >
          <button
            ref="closeFinanceButton"
            type="button"
            class="absolute right-1 top-1 rounded p-1 text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
            aria-label="关闭价值详情"
            @click.stop="closeFinanceCard"
          >
            <Icon icon="lucide:x" :width="14" :height="14" />
          </button>
          <div class="flex h-full min-w-0 flex-col overflow-hidden">
            <div class="shrink-0 grid grid-cols-3 gap-1.5">
              <div v-for="(item, index) in financeSummaryItems" :key="item.label" class="min-w-0">
                <div class="flex mb-1.5 items-center text-xs font-medium text-muted-foreground">
                  {{ item.label }}
                </div>
                <Transition v-bind="metricSwitchTransitionProps">
                  <div
                    :key="`finance-${summaryTransitionKey}-${item.label}`" class="flex min-w-0 items-baseline truncate"
                    :style="getMetricSwitchStyle(index)"
                  >
                    <span class="shrink-0 text-xs mr-0.5 font-semibold leading-none text-muted-foreground">
                      {{ item.symbol }}
                    </span>
                    <span class=" text-sm md:text-lg font-bold leading-none tracking-tight">
                      {{ item.value }}
                    </span>
                  </div>
                </Transition>
              </div>
            </div>
            <div class="flex-1 my-1.5" />
            <div class="shrink-0 flex flex-col flex-1">
              <div class="flex mb-1 items-center justify-between gap-2">
                <div class="flex items-center gap-1 text-xs font-medium tracking-wider text-muted-foreground">
                  {{ dailyRates ? `汇率 · ${rateDate}` : '参考汇率估算' }}
                </div>
                <select v-model="financeCurrency" aria-label="切换汇率基准币种" class="bg-background text-xs rounded border border-border p-1">
                  <option v-for="currency in FINANCE_CURRENCIES.filter(code => rates[code])" :key="currency" :value="currency">
                    {{ currency }}
                  </option>
                </select>
              </div>
              <div class="h-15 grid grid-cols-2 gap-y-1 gap-x-4 overflow-auto">
                <div v-for="row in exchangeRateRows" :key="row.currency" class="text-[11px] flex justify-between gap-2">
                  <span class="text-muted-foreground">{{ row.currency }}</span>
                  <span>{{ row.symbol }}{{ row.rate }}</span>
                </div>
              </div>
              <p class="text-[10px] text-muted-foreground mt-1">
                {{ dailyRates ? 'Frankfurter 每日汇率；休市时显示最近交易日。' : '当前使用主题内置参考汇率，非实时。可在站点设置启用每日汇率。' }} 月均按 30 天折算；剩余按未到期时长折算。
              </p>
              <p v-if="finance.excluded" class="text-[10px] text-muted-foreground">
                {{ finance.excluded }} 个节点因资料不完整，未计入部分估算。
              </p>
            </div>
          </div>
        </CardX>
      </div>
      <CardX
        hoverable
        class="group h-full border-none rounded-md transition-all"
        :class="[
          pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs'),
          showVisualPanel ? 'col-span-4 row-span-1 col-start-5 row-start-2' : 'col-span-1 row-start-2 col-start-2 min-h-18 md:min-h-24 md:row-start-1 md:col-start-4',
        ]"
        content-class="h-full !p-3"
      >
        <div class="flex h-full flex-col justify-between gap-1">
          <div class="flex items-start justify-between">
            <span class="text-xs font-medium tracking-wider text-muted-foreground">累计流量</span>
            <Icon
              icon="tabler:download" :width="20" :height="20"
              class="text-slate-500/20 group-hover:text-slate-500 transition-colors"
            />
          </div>
          <DataTooltip
            as="span" placement="top"
            :content="`↑ ${formattedTrafficUp.value} ${formattedTrafficUp.unit}\n↓ ${formattedTrafficDown.value} ${formattedTrafficDown.unit}`"
            class="min-w-0" content-class="whitespace-pre px-2 py-1 left-0 -translate-x-0 leading-normal"
          >
            <Transition v-bind="metricSwitchTransitionProps">
              <div
                :key="`traffic-${summaryTransitionKey}`" class="flex items-baseline gap-1"
                :style="getMetricSwitchStyle(3)"
              >
                <span class="inline-block text-md md:text-2xl font-bold leading-none tracking-tight">
                  {{ totalTrafficTooltip.value }}
                </span>
                <span class="inline-block text-[11px] md:text-xs font-medium text-muted-foreground">
                  {{ totalTrafficTooltip.unit }}
                </span>
              </div>
            </Transition>
          </DataTooltip>
        </div>
      </CardX>

      <CardX
        hoverable
        class="group h-full border-none rounded-md transition-all"
        :class="[
          pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs'),
          showVisualPanel ? 'col-span-4 row-span-1 col-start-9 row-start-1' : 'col-span-1 row-start-1 col-start-3 min-h-18 md:min-h-24 md:row-start-1 md:col-start-5',
        ]"
        content-class="h-full !p-3"
      >
        <div class="flex h-full flex-col justify-between gap-1">
          <div class="flex items-start justify-between">
            <span class="text-xs font-medium tracking-wider text-muted-foreground">实时上行</span>
            <Icon
              icon="tabler:chevrons-up" :width="20" :height="20"
              class="text-slate-500/20 group-hover:text-slate-500 transition-colors"
            />
          </div>
          <Transition v-bind="metricSwitchTransitionProps">
            <div
              :key="`speed-up-${summaryTransitionKey}`" class="flex items-baseline gap-1"
              :style="getMetricSwitchStyle(4)"
            >
              <span class="text-md md:text-2xl font-bold leading-none tracking-tight">{{ formattedSpeedUp.value
              }}</span>
              <span class="text-[11px] md:text-xs font-medium text-muted-foreground">{{ formattedSpeedUp.unit }}</span>
            </div>
          </Transition>
        </div>
      </CardX>
      <CardX
        hoverable
        class="group h-full border-none rounded-md transition-all"
        :class="[
          pickSurfaceClass('bg-background/60 hover:bg-background', 'bg-background/50 hover:bg-background backdrop-blur-xs'),
          showVisualPanel ? 'col-span-4 row-span-1 col-start-9 row-start-2' : 'col-span-1 row-start-2 col-start-3 min-h-18 md:min-h-24 md:row-start-1 md:col-start-6',
        ]"
        content-class="h-full !p-3"
      >
        <div class="flex h-full flex-col justify-between gap-1">
          <div class="flex items-start justify-between">
            <span class="text-xs font-medium tracking-wider text-muted-foreground">实时下行</span>
            <Icon
              icon="tabler:chevrons-down" :width="20" :height="20"
              class="text-slate-500/20 group-hover:text-slate-500 transition-colors"
            />
          </div>
          <Transition v-bind="metricSwitchTransitionProps">
            <div
              :key="`speed-down-${summaryTransitionKey}`" class="flex items-baseline gap-1"
              :style="getMetricSwitchStyle(5)"
            >
              <span class="text-md md:text-2xl font-bold leading-none tracking-tight">
                {{ formattedSpeedDown.value }}
              </span>
              <span class="text-[11px] md:text-xs font-medium text-muted-foreground">{{ formattedSpeedDown.unit
              }}</span>
            </div>
          </Transition>
        </div>
      </CardX>
    </div>
  </div>
</template>

<style scoped>
.metric-switch-enter-active,
.metric-switch-leave-active {
  transition:
    opacity 160ms ease,
    transform 180ms cubic-bezier(0.22, 1, 0.36, 1),
    filter 180ms ease;
}

.metric-switch-enter-active {
  transition-delay: var(--metric-switch-delay, 0ms);
}

.metric-switch-enter-from {
  opacity: 0;
  transform: translateY(6px);
  filter: blur(3px);
}

.metric-switch-leave-to {
  opacity: 0;
  transform: translateY(-4px);
  filter: blur(2px);
}

@media (prefers-reduced-motion: reduce) {
  .metric-switch-enter-active,
  .metric-switch-leave-active {
    transition: none;
    transition-delay: 0ms;
  }

  .metric-switch-enter-from,
  .metric-switch-leave-to {
    opacity: 1;
    transform: none;
    filter: none;
  }
}
</style>
