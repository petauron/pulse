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
