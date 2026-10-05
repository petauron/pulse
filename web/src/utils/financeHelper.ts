import type { NodeData } from '@/stores/nodes'

const CURRENCY_SYMBOL_CONFIG = {
  AUD: 'A$',
  BRL: 'R$',
  CAD: 'C$',
  CHF: 'CHF',
  CNY: '¥',
  CZK: 'Kč',
  DKK: 'kr',
  EUR: '€',
  GBP: '£',
  HKD: '$',
  HUF: 'Ft',
  IDR: 'Rp',
  ILS: '₪',
  INR: '₹',
  ISK: 'kr',
  JPY: '¥',
  KRW: '₩',
  KZT: '₸',
  MXN: 'Mex$',
  MYR: 'RM',
  NOK: 'kr',
  NZD: 'NZ$',
  PHP: '₱',
  PLN: 'zł',
  RON: 'lei',
  RUB: '₽',
  SEK: 'kr',
  SGD: 'S$',
  THB: '฿',
  TRY: '₺',
  UAH: '₴',
  USD: '$',
  VND: '₫',
  ZAR: 'R',
} as const

export type CurrencyCode = keyof typeof CURRENCY_SYMBOL_CONFIG

export const CURRENCY_SYMBOLS: Record<CurrencyCode, string> = CURRENCY_SYMBOL_CONFIG

const EXPLICIT_CURRENCY_ALIASES: Record<string, CurrencyCode> = {
  '$': 'USD',
  'US$': 'USD',
  'CA$': 'CAD',
  'CN¥': 'CNY',
  'RMB': 'CNY',
  'HK$': 'HKD',
  '€': 'EUR',
  '£': 'GBP',
  '¥': 'CNY',
  '￥': 'CNY',
  'JP¥': 'JPY',
}

export function normalizeCurrency(currency: string | null | undefined): CurrencyCode {
  const value = String(currency || 'CNY').trim().toUpperCase()
  if (value in CURRENCY_SYMBOL_CONFIG)
    return value as CurrencyCode
  return EXPLICIT_CURRENCY_ALIASES[value] || 'CNY'
}

// Emerald-Cazi bundled reference rates: units per CNY, not live market quotes.
export const REFERENCE_EXCHANGE_RATES: Record<CurrencyCode, number> = {
  AUD: 0.20941,
  BRL: 0.74734,
  CAD: 0.20691,
  CHF: 0.11746,
  CNY: 1,
  CZK: 3.0787,
  DKK: 0.95296,
  EUR: 0.1275,
  GBP: 0.11027,
  HKD: 1.1594,
  HUF: 44.688,
  IDR: 2622.37,
  ILS: 0.43085,
  INR: 14.0178,
  ISK: 18.4626,
  JPY: 23.707,
  KRW: 224.11,
  KZT: 64,
  MXN: 2.5472,
  MYR: 0.59945,
  NOK: 1.4096,
  NZD: 0.2535,
  PHP: 8.9288,
  PLN: 0.54138,
  RON: 0.66769,
  RUB: 11.9,
  SEK: 1.3895,
  SGD: 0.18975,
  THB: 4.8172,
  TRY: 6.849,
  UAH: 3.6,
  USD: 0.14799,
  VND: 3500,
  ZAR: 2.3995,
}

export const FINANCE_CURRENCIES = ['CNY', ...Object.keys(CURRENCY_SYMBOLS).filter(code => code !== 'CNY')] as CurrencyCode[]

type PricedNode = Pick<NodeData, 'price' | 'billing_cycle' | 'currency' | 'expired_at' | 'tags'>

export function summarizeNodeValue(nodes: PricedNode[], now = Date.now()) {
  let total = 0
  let monthly = 0
  let remaining = 0
  let excluded = 0
  for (const node of nodes) {
    if (!Number.isFinite(node.price) || node.price <= 0 || node.tags?.includes('白嫖中'))
      continue
    const rawCurrency = String(node.currency || 'CNY').trim().toUpperCase()
    if (!(rawCurrency in CURRENCY_SYMBOLS) && !(rawCurrency in EXPLICIT_CURRENCY_ALIASES)) {
      excluded++
      continue
    }
    const price = node.price / REFERENCE_EXCHANGE_RATES[normalizeCurrency(node.currency)]
    total += price
    const cycle = Number(node.billing_cycle)
    if (Number.isFinite(cycle) && cycle > 0)
      monthly += price / cycle * 30
    const expiry = Date.parse(node.expired_at)
    if (!Number.isFinite(expiry)) {
      excluded++
      continue
    }
    const days = (expiry - now) / 86400000
    if (days > 36500)
      remaining += price
    else if (days > 0 && Number.isFinite(cycle) && cycle > 0)
      remaining += price * days / cycle
    else if (days > 0)
      excluded++
  }
  return { total, monthly, remaining, excluded }
}
