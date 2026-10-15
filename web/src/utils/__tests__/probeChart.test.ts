import { describe, expect, it } from 'vitest'
import { probeChartSeries } from '../probeChart'

const start = Date.parse('2026-01-01T00:00:00.000Z')
const minute = 60_000
const point = (bucket: number, latency: number | null) => ({ received_at_unix_ms: start + bucket * minute + 10_000, latency_ms: latency })

describe('probeChartSeries', () => {
  it('keeps a long reporting gap empty instead of drawing a continuous line', () => {
    const points = [point(0, 10), point(1, 11), point(2, 12), point(52, 13)]
    const series = probeChartSeries(points, start, start + 60 * minute, minute, false)
    const smoothed = probeChartSeries(points, start, start + 60 * minute, minute, true)

    expect(series).toHaveLength(53)
    expect(series[0]?.[1]).toBe(10)
    expect(series[3]?.[1]).toBeNull()
    expect(series[51]?.[1]).toBeNull()
    expect(series[52]?.[1]).toBe(13)
    expect(smoothed[3]?.[1]).toBeNull()
    expect(smoothed[51]?.[1]).toBeNull()
  })

  it('uses Emerald bounded interpolation for a short failure gap without changing source samples', () => {
    const points = [point(0, 10), point(1, null), point(2, 20)]
    const series = probeChartSeries(points, start, start + 60 * minute, minute, false)

    expect(series).toHaveLength(3)
    expect(series[1]?.[1]).toBeCloseTo(15)
    expect(points[1]?.latency_ms).toBeNull()
  })

  it('does not invent values before the first or after the last received bucket', () => {
    const series = probeChartSeries([point(10, 8), point(11, 9)], start, start + 60 * minute, minute, false)

    expect(series).toHaveLength(2)
    expect(series[0]?.[0]).toBe(point(10, 8).received_at_unix_ms)
    expect(series[1]?.[0]).toBe(point(11, 9).received_at_unix_ms)
  })

  it('rejects an oversized or malformed bucket range', () => {
    expect(probeChartSeries([point(0, 10)], start, start + 100_000 * minute, minute, false)).toEqual([])
    expect(probeChartSeries([point(0, 10)], start, start + minute, 0, false)).toEqual([])
  })
})
