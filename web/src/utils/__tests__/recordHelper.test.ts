import type { RecordFormat } from '../recordHelper'
import { describe, expect, it } from 'vitest'
import { alignRecordsToTimeWindow } from '../recordHelper'

const minute = 60_000
const start = Date.parse('2026-01-01T00:00:00.000Z')

function record(minuteOffset: number, cpu: number | null): RecordFormat {
  return {
    client: 'node-1',
    time: new Date(start + minuteOffset * minute).toISOString(),
    cpu,
    gpu: null,
    gpu_usage: null,
    gpu_memory: null,
    ram: cpu,
    ram_total: cpu,
    swap: cpu,
    swap_total: cpu,
    load: cpu,
    temp: null,
    disk: cpu,
    disk_total: cpu,
    net_in: cpu,
    net_out: cpu,
    net_total_up: cpu,
    net_total_down: cpu,
    process: cpu,
    connections: cpu,
    connections_udp: cpu,
  }
}

const emptyRecord = record(0, null)

describe('alignRecordsToTimeWindow', () => {
  it('keeps the offline tail as null through the requested end', () => {
    const rows = alignRecordsToTimeWindow(
      [record(5, 5), record(15, 15), record(25, 25)],
      10 * minute,
      start,
      start + 60 * minute,
      emptyRecord,
    )

    expect(rows).toHaveLength(6)
    expect(rows.map(row => row.cpu)).toEqual([5, 15, 25, null, null, null])
    expect(rows.at(-1)?.time).toBe(new Date(start + 60 * minute).toISOString())
  })

  it('keeps the offline head and sparse middle buckets as null', () => {
    const rows = alignRecordsToTimeWindow(
      [record(25, 25), record(55, 55)],
      10 * minute,
      start,
      start + 60 * minute,
      emptyRecord,
    )

    expect(rows.map(row => row.cpu)).toEqual([null, null, 25, null, null, 55])
  })

  it('selects one nearest sample per bucket without reusing old values', () => {
    const rows = alignRecordsToTimeWindow(
      [record(1, 1), record(9, 9)],
      10 * minute,
      start,
      start + 30 * minute,
      emptyRecord,
    )

    expect(rows.map(row => row.cpu)).toEqual([9, null, null])
  })

  it('creates an all-null requested window when there are no samples', () => {
    const rows = alignRecordsToTimeWindow(
      [],
      10 * minute,
      start,
      start + 30 * minute,
      emptyRecord,
    )

    expect(rows.map(row => row.cpu)).toEqual([null, null, null])
    expect(rows.map(row => row.time)).toEqual([
      new Date(start + 10 * minute).toISOString(),
      new Date(start + 20 * minute).toISOString(),
      new Date(start + 30 * minute).toISOString(),
    ])
  })
})
