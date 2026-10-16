import { describe, expect, it } from 'vitest'
import { failureRateLabel, failureRateLabelForKinds } from '../probeLabels'

describe('probe failure labels', () => {
  it('uses Emerald ping-loss labels for ICMP and TCP while keeping HTTP failures distinct', () => {
    expect(failureRateLabel('icmp')).toBe('丢包率')
    expect(failureRateLabel('tcp')).toBe('丢包率')
    expect(failureRateLabel('tcp', true)).toBe('丢包')
    expect(failureRateLabel('http', true)).toBe('失败')
  })

  it('names mixed charts without conflating their metrics', () => {
    expect(failureRateLabelForKinds(['icmp', 'tcp'])).toBe('丢包率')
    expect(failureRateLabelForKinds(['tcp', 'http'], true)).toBe('丢包 / 失败')
    expect(failureRateLabelForKinds([])).toBe('丢包率 / 失败率')
  })
})
