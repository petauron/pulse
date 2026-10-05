import { describe, expect, it } from 'vitest'
import { REFERENCE_EXCHANGE_RATES, summarizeNodeValue } from '../financeHelper'

const now = Date.parse('2026-10-01T00:00:00Z')
const paid = { price: 30, billing_cycle: 30, currency: 'CNY', expired_at: '2026-10-16T00:00:00Z', tags: '' }

describe('node value summary', () => {
  it('converts mixed currencies before aggregating unused days and monthly cost', () => {
    const summary = summarizeNodeValue([paid, { ...paid, price: 30 * REFERENCE_EXCHANGE_RATES.USD, currency: 'USD' }], now)
    expect(summary.total).toBeCloseTo(60)
    expect(summary.monthly).toBeCloseTo(60)
    expect(summary.remaining).toBeCloseTo(30)
  })

  it('counts expired plans as zero remaining and excludes free-tagged plans', () => {
    const summary = summarizeNodeValue([{ ...paid, expired_at: '2026-09-30T00:00:00Z' }, { ...paid, tags: '白嫖中' }], now)
    expect(summary).toEqual({ total: 30, monthly: 30, remaining: 0, excluded: 0 })
  })

  it('does not silently treat unsupported currency or missing expiry as a complete estimate', () => {
    const summary = summarizeNodeValue([{ ...paid, currency: 'UNKNOWN' }, { ...paid, expired_at: '' }], now)
    expect(summary).toEqual({ total: 30, monthly: 30, remaining: 0, excluded: 2 })
  })

  it('keeps lifetime value and avoids dividing by invalid billing periods', () => {
    const summary = summarizeNodeValue([{ ...paid, expired_at: '9999-01-01T00:00:00Z', billing_cycle: 0 }], now)
    expect(summary).toEqual({ total: 30, monthly: 0, remaining: 30, excluded: 0 })
  })
})
