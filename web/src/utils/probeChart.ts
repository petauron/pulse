import { cutPeakValues, interpolateNullsLinear } from './recordHelper'

export interface ProbeChartPoint {
  received_at_unix_ms: number
  latency_ms: number | null
}

export interface ProbeFailurePoint extends ProbeChartPoint {
  failed_samples: number
  samples: number
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

/** Plot the observed failure ratio in each bucket; never turn missing reports into 0%. */
export function probeFailureSeries(
  points: readonly ProbeFailurePoint[],
  rangeStartMs: number,
  rangeEndMs: number,
  bucketMs: number,
): Array<[number, number | null]> {
  if (!points.length || !Number.isFinite(rangeStartMs) || !Number.isFinite(rangeEndMs) || !Number.isSafeInteger(bucketMs) || bucketMs < 1 || rangeEndMs <= rangeStartMs)
    return []

  const bucketCount = Math.ceil((rangeEndMs - rangeStartMs + 1) / bucketMs)
  if (bucketCount > MAX_PROBE_CHART_BUCKETS)
    return []
  const byBucket = new Map<number, ProbeFailurePoint>()
  for (const point of points) {
    const index = Math.floor((point.received_at_unix_ms - rangeStartMs) / bucketMs)
    if (!Number.isFinite(index) || index < 0 || index >= bucketCount || !Number.isInteger(point.samples) || point.samples < 1 || !Number.isInteger(point.failed_samples) || point.failed_samples < 0 || point.failed_samples > point.samples)
      continue
    const current = byBucket.get(index)
    if (!current || point.received_at_unix_ms >= current.received_at_unix_ms)
      byBucket.set(index, point)
  }
  const observed = [...byBucket.values()].sort((a, b) => a.received_at_unix_ms - b.received_at_unix_ms)
  if (!observed.length)
    return []

  const gaps = observed.slice(1).map((point, index) => point.received_at_unix_ms - observed[index]!.received_at_unix_ms).sort((a, b) => a - b)
  const typicalGapMs = gaps[Math.floor(gaps.length / 2)] ?? bucketMs
  const maxGapMs = Math.max(2 * 60_000, Math.min(Math.max(30 * 60_000, bucketMs * 6), typicalGapMs * 6))
  const series: Array<[number, number | null]> = []
  for (const point of observed) {
    const previous = series.at(-1)
    if (previous && point.received_at_unix_ms - previous[0] > maxGapMs)
      series.push([previous[0] + bucketMs, null])
    series.push([point.received_at_unix_ms, point.failed_samples * 100 / point.samples])
  }
  return series
}
