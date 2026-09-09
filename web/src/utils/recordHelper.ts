/**
 * 数据处理工具函数
 * 参考 React 版本的 RecordHelper.tsx 实现
 */

import dayjs from 'dayjs'

/** 负载记录格式 */
export interface RecordFormat {
  client: string
  time: string
  cpu: number | null
  gpu: number | null
  gpu_usage: number | null
  gpu_memory: number | null
  gpu_detailed?: {
    [index: number]: {
      usage: number | null
      memory: number | null
      temperature: number | null
      device_index?: number
      device_name?: string
      mem_total?: number
      mem_used?: number
    }
  }
  ram: number | null
  ram_total: number | null
  swap: number | null
  swap_total: number | null
  load: number | null
  temp: number | null
  disk: number | null
  disk_total: number | null
  net_in: number | null
  net_out: number | null
  net_total_up: number | null
  net_total_down: number | null
  process: number | null
  connections: number | null
  connections_udp: number | null
}

type AnyRecord = Record<string, any>

/**
 * 创建空值模板
 * 递归设置所有数值属性为 null，用于填充缺失的时间点
 */
function createNullTemplate(obj: unknown): unknown {
  if (obj === null || obj === undefined)
    return null
  if (typeof obj === 'number')
    return null
  if (typeof obj === 'string' || typeof obj === 'boolean')
    return obj
  if (Array.isArray(obj))
    return obj.map(createNullTemplate)
  if (typeof obj === 'object') {
    const res: Record<string, unknown> = {}
    for (const k in obj as Record<string, unknown>) {
      if (k === 'updated_at' || k === 'time')
        continue
      res[k] = createNullTemplate((obj as Record<string, unknown>)[k])
    }
    return res
  }
  return null
}

/**
 * 将记录映射到 Service 声明的固定时间窗。
 *
 * 每个输入样本只属于一个半开时间桶；一个桶有多个样本时选择最接近桶末的样本。
 * 没有样本的桶保留 null 数值，避免把离线区段绘制成零值或重复旧样本。
 */
export function alignRecordsToTimeWindow<T extends { time?: string, updated_at?: string }>(
  data: T[],
  intervalMs: number,
  requestedStartUnixMs: number,
  requestedEndUnixMs: number,
  emptyTemplate?: T,
): T[] {
  if (
    !Number.isFinite(intervalMs)
    || intervalMs <= 0
    || !Number.isFinite(requestedStartUnixMs)
    || !Number.isFinite(requestedEndUnixMs)
    || requestedEndUnixMs <= requestedStartUnixMs
  ) {
    return []
  }

  const getTime = (item: T) =>
    dayjs(item.time ?? item.updated_at ?? '').valueOf()

  const timedData = data
    .map(item => ({ item, timeMs: getTime(item) }))
    .filter(({ timeMs }) =>
      Number.isFinite(timeMs)
      && timeMs >= requestedStartUnixMs
      && timeMs <= requestedEndUnixMs,
    )
  timedData.sort((a, b) => a.timeMs - b.timeMs)

  const lastItem = timedData.at(-1)
  const template = emptyTemplate ?? lastItem?.item
  if (!template)
    return []

  const bucketCount = Math.max(
    1,
    Math.ceil((requestedEndUnixMs - requestedStartUnixMs) / intervalMs),
  )
  const selected: Array<{ item: T, timeMs: number } | undefined> = Array.from({
    length: bucketCount,
  })
  for (const candidate of timedData) {
    const elapsed = Math.max(0, candidate.timeMs - requestedStartUnixMs)
    const bucketIndex = Math.min(
      bucketCount - 1,
      Math.max(0, Math.ceil(elapsed / intervalMs) - 1),
    )
    const bucketEnd = Math.min(
      requestedEndUnixMs,
      requestedStartUnixMs + (bucketIndex + 1) * intervalMs,
    )
    const current = selected[bucketIndex]
    if (
      !current
      || Math.abs(candidate.timeMs - bucketEnd) <= Math.abs(current.timeMs - bucketEnd)
    ) {
      selected[bucketIndex] = candidate
    }
  }

  const nullTemplate = createNullTemplate(template) as T
  return selected.map((candidate, index) => {
    const bucketEnd = Math.min(
      requestedEndUnixMs,
      requestedStartUnixMs + (index + 1) * intervalMs,
    )
    return {
      ...(candidate?.item ?? nullTemplate),
      time: dayjs(bucketEnd).toISOString(),
    }
  })
}

/**
 * 线性插值填充
 * 在相邻两个有效点之间，用线性插值填充中间的 null 值
 * - 仅在"两个端点都存在且为数值"时进行插值
 * - 可通过 maxGapMs 控制最大可插值的时间跨度
 */
export function interpolateNullsLinear(
  rows: AnyRecord[],
  keys: string[],
  options?:
    | number
    | {
      /** 统一的最大插值跨度 */
      maxGapMs?: number
      /** 若未提供 maxGapMs，则以典型间隔 * 该倍数作为最大插值跨度 */
      maxGapMultiplier?: number
      /** 统一的下限与上限（用于钳制） */
      minCapMs?: number
      maxCapMs?: number
    },
): AnyRecord[] {
  if (!rows || rows.length === 0 || !keys.length)
    return rows

  const times = rows.map(r =>
    dayjs(r.time ?? r.updated_at ?? '').valueOf(),
  )
  const out: AnyRecord[] = rows.map(r => ({ ...r }))

  // 解析配置
  const opts
    = typeof options === 'number'
      ? { maxGapMs: options }
      : options || {}
  const maxGapMsUnified = opts.maxGapMs
  const multiplier = opts.maxGapMultiplier ?? 6
  const minCap = opts.minCapMs ?? 2 * 60_000 // 2min
  const maxCap = opts.maxCapMs ?? 30 * 60_000 // 30min

  const clamp = (v: number, lo: number, hi: number) =>
    Math.max(lo, Math.min(hi, v))

  for (const key of keys) {
    // 收集该列的有效点索引
    const validIdx: number[] = []
    for (let i = 0; i < rows.length; i++) {
      const v = rows[i]?.[key]
      if (typeof v === 'number' && Number.isFinite(v))
        validIdx.push(i)
    }

    if (validIdx.length < 2)
      continue

    // 计算该列的"典型间隔"（使用中位数）
    let perKeyMaxGap = maxGapMsUnified
    if (perKeyMaxGap === undefined) {
      const gaps: number[] = []
      for (let s = 0; s < validIdx.length - 1; s++) {
        const i0 = validIdx[s]
        const i1 = validIdx[s + 1]
        if (i0 === undefined || i1 === undefined)
          continue
        const t0 = times[i0]
        const t1 = times[i1]
        if (t0 !== undefined && t1 !== undefined && Number.isFinite(t0) && Number.isFinite(t1) && t1 > t0) {
          gaps.push(t1 - t0)
        }
      }
      if (gaps.length === 0)
        continue
      gaps.sort((a, b) => a - b)
      const median = gaps[Math.floor(gaps.length / 2)]
      if (median === undefined)
        continue
      perKeyMaxGap = clamp(median * multiplier, minCap, maxCap)
    }

    // 相邻有效点之间做线性插值
    for (let s = 0; s < validIdx.length - 1; s++) {
      const i0 = validIdx[s]
      const i1 = validIdx[s + 1]
      if (i0 === undefined || i1 === undefined)
        continue

      const t0 = times[i0]
      const t1 = times[i1]
      if (t0 === undefined || t1 === undefined)
        continue

      const row0 = rows[i0]
      const row1 = rows[i1]
      if (!row0 || !row1)
        continue

      const v0 = row0[key]
      const v1 = row1[key]

      if (!Number.isFinite(t0) || !Number.isFinite(t1) || t1 <= t0)
        continue
      if (typeof v0 !== 'number' || typeof v1 !== 'number')
        continue
      if (perKeyMaxGap && t1 - t0 > perKeyMaxGap)
        continue // 间隔太大，保持空洞

      for (let j = i0 + 1; j < i1; j++) {
        const tj = times[j]
        if (tj === undefined)
          continue
        const ratio = (tj - t0) / (t1 - t0)
        const outRow = out[j]
        if (outRow) {
          outRow[key] = v0 + (v1 - v0) * ratio
        }
      }
    }
  }

  return out
}

/**
 * EWMA（指数加权移动平均）峰值裁剪
 * 使用指数加权移动平均算法平滑数据，同时检测并过滤突变值，填充 null/undefined 值
 *
 * @param data 输入数据数组
 * @param keys 需要处理的数值属性名数组
 * @param alpha 平滑因子
 * @param windowSize 突变检测窗口大小
 * @param spikeThreshold 突变阈值
 */
export function cutPeakValues(
  data: AnyRecord[],
  keys: string[],
  alpha: number = 0.3,
  windowSize: number = 15,
  spikeThreshold: number = 0.3,
): AnyRecord[] {
  if (!data || data.length === 0)
    return data

  const result: AnyRecord[] = [...data]
  const halfWindow = Math.floor(windowSize / 2)

  for (const key of keys) {
    // 第一步：检测并移除突变值
    for (let i = 0; i < result.length; i++) {
      const currentRow = result[i]
      if (!currentRow)
        continue
      const currentValue = currentRow[key]

      if (currentValue != null && typeof currentValue === 'number') {
        const neighborValues: number[] = []

        // 收集窗口范围内的邻近有效值
        for (
          let j = Math.max(0, i - halfWindow);
          j <= Math.min(result.length - 1, i + halfWindow);
          j++
        ) {
          if (j === i)
            continue
          const neighborRow = result[j]
          if (!neighborRow)
            continue
          const neighbor = neighborRow[key]
          if (neighbor != null && typeof neighbor === 'number') {
            neighborValues.push(neighbor)
          }
        }

        // 如果有足够的邻近值进行突变检测
        if (neighborValues.length >= 2) {
          const neighborSum = neighborValues.reduce((sum, val) => sum + val, 0)
          const neighborMean = neighborValues.length > 0 ? neighborSum / neighborValues.length : 0

          // 检测突变
          if (neighborMean > 0) {
            const relativeChange = Math.abs(currentValue - neighborMean) / neighborMean
            if (relativeChange > spikeThreshold) {
              result[i] = { ...currentRow, [key]: null }
            }
          }
          else if (Math.abs(currentValue) > 10) {
            result[i] = { ...currentRow, [key]: null }
          }
        }
      }
    }

    // 第二步：使用 EWMA 平滑和填充
    let ewma: number | null = null

    for (let i = 0; i < result.length; i++) {
      const row = result[i]
      if (!row)
        continue
      const currentValue = row[key]

      if (currentValue != null && typeof currentValue === 'number') {
        if (ewma === null) {
          ewma = currentValue
        }
        else {
          ewma = alpha * currentValue + (1 - alpha) * ewma
        }
        result[i] = { ...row, [key]: ewma }
      }
      else if (ewma !== null) {
        result[i] = { ...row, [key]: ewma }
      }
    }
  }

  return result
}
