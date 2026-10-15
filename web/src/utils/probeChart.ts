import { cutPeakValues, interpolateNullsLinear } from './recordHelper'

export interface ProbeChartPoint {
  received_at_unix_ms: number
  latency_ms: number | null
}

const MAX_PROBE_CHART_BUCKETS = 1_000

/** Keep empty time buckets visible without connecting across long reporting gaps. */
export function probeChartSeries(
  points: readonly ProbeChartPoint[],
  rangeStartMs: number,
  rangeEndMs: number,
  bucketMs: number,
  cutPeak: boolean,
): Array<[number, number | null]> {
  if (!points.length || !Number.isFinite(rangeStartMs) || !Number.isFinite(rangeEndMs) || !Number.isSafeInteger(bucketMs) || bucketMs < 1 || rangeEndMs <= rangeStartMs)
    return []

  const bucketCount = Math.ceil((rangeEndMs - rangeStartMs + 1) / bucketMs)
  if (bucketCount > MAX_PROBE_CHART_BUCKETS)
    return []
  const byBucket = new Map<number, ProbeChartPoint>()
  for (const point of points) {
    const index = Math.floor((point.received_at_unix_ms - rangeStartMs) / bucketMs)
    if (index >= 0 && index < bucketCount)
      byBucket.set(index, point)
  }
  if (!byBucket.size)
    return []

  const indexes = [...byBucket.keys()]
  const first = Math.min(...indexes)
  const last = Math.max(...indexes)
  const rows = Array.from({ length: last - first + 1 }, (_, offset) => {
    const index = first + offset
    const point = byBucket.get(index)
    const time = point?.received_at_unix_ms ?? Math.min(rangeEndMs, rangeStartMs + (index + 0.5) * bucketMs)
    return { time: new Date(time).toISOString(), latency: point?.latency_ms ?? null }
  })
  const options = {
    maxGapMultiplier: 6,
    minCapMs: 2 * 60_000,
    maxCapMs: Math.max(30 * 60_000, bucketMs * 6),
  }
  const rawSeries = interpolateNullsLinear(rows, ['latency'], options)
  const series = cutPeak
    ? interpolateNullsLinear(cutPeakValues(rows, ['latency']), ['latency'], options)
    : rawSeries
  if (cutPeak) {
    for (let index = 0; index < series.length; index++) {
      if (rawSeries[index]?.latency == null)
        series[index]!.latency = null
    }
  }
  return series.map(row => [Date.parse(row.time), typeof row.latency === 'number' ? row.latency : null])
}
