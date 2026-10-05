import type { CurrencyCode } from '@/utils/financeHelper'
import { useNow } from '@vueuse/core'
import { computed, onBeforeUnmount, shallowRef, watch } from 'vue'
import { useAppStore } from '@/stores/app'
import { CURRENCY_SYMBOLS, REFERENCE_EXCHANGE_RATES } from '@/utils/financeHelper'

const cacheKey = 'pulse:frankfurter-cny:v1'
type Rates = Partial<Record<CurrencyCode, number>>
function parseRates(raw: unknown): Rates | null {
  if (!raw || typeof raw !== 'object')
    return null
  const rates: Rates = { CNY: 1 }
  for (const code of Object.keys(CURRENCY_SYMBOLS) as CurrencyCode[]) {
    const rate = (raw as Record<string, unknown>)[code]
    if (typeof rate === 'number' && Number.isFinite(rate) && rate > 0)
      rates[code] = rate
  }
  return rates.USD && rates.EUR ? rates : null
}

export function useExchangeRates() {
  const app = useAppStore()
  const rates = shallowRef<Rates>(REFERENCE_EXCHANGE_RATES)
  const date = shallowRef('')
  const live = computed(() => Boolean(date.value))
  const clock = useNow({ interval: 60000 })
  const day = computed(() => clock.value.toISOString().slice(0, 10))
  let controller: AbortController | undefined
  watch(() => [app.publicSettings?.theme_settings?.dailyExchangeRates, day.value] as const, async ([enabled, today]) => {
    controller?.abort()
    rates.value = REFERENCE_EXCHANGE_RATES
    date.value = ''
    if (enabled !== true)
      return
    try {
      const raw = localStorage.getItem(cacheKey)
      if (raw && raw.length < 8192) {
        const cache = JSON.parse(raw)
        const parsed = parseRates(cache.rates)
        if (cache.fetched === today && typeof cache.date === 'string' && parsed) {
          rates.value = parsed
          date.value = cache.date
          return
        }
      }
    }
    catch { /* Storage is optional; never prevent rendering. */ }
    const request = new AbortController()
    controller = request
    const timer = setTimeout(() => request.abort(), 5000)
    try {
      const response = await fetch('https://api.frankfurter.dev/v1/latest?base=CNY', { signal: request.signal, credentials: 'omit', referrerPolicy: 'no-referrer' })
      if (!response.ok)
        return
      const payload = await response.json()
      const parsed = parseRates(payload.rates)
      if (request.signal.aborted || !parsed || payload.base !== 'CNY' || !/^\d{4}-\d{2}-\d{2}$/.test(payload.date))
        return
      rates.value = parsed
      date.value = payload.date
      try { localStorage.setItem(cacheKey, JSON.stringify({ fetched: today, date: payload.date, rates: parsed })) }
      catch { /* Private browsing may disable storage. */ }
    }
    catch { /* Keep the explicitly labelled reference estimate on failure. */ }
    finally { clearTimeout(timer) }
  }, { immediate: true })
  onBeforeUnmount(() => controller?.abort())
  return { rates, date, live }
}
