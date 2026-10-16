import { describe, expect, it } from 'vitest'
import { failureRateLabel, failureRateLabelForKinds } from '../probeLabels'

describe('probe failure labels', () => {
  it('distinguishes ICMP packet loss from TCP and HTTP probe failures', () => {
    expect(failureRateLabel('icmp')).toBe('丢包率')
    expect(failureRateLabel('tcp')).toBe('失败率')
    expect(failureRateLabel('http', true)).toBe('失败')
  })

  it('names mixed charts without conflating their metrics', () => {
    expect(failureRateLabelForKinds(['icmp', 'tcp'])).toBe('丢包率 / 失败率')
    expect(failureRateLabelForKinds(['tcp', 'http'], true)).toBe('失败')
    expect(failureRateLabelForKinds([])).toBe('丢包率 / 失败率')
  })
})
