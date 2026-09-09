import { describe, expect, it } from 'vitest'
import { summarizeNodeCapacity } from '../nodeSummary'

describe('summarizeNodeCapacity', () => {
  it('excludes retained offline samples from current usage', () => {
    const summary = summarizeNodeCapacity([
      {
        online: true,
        ram: 4,
        mem_total: 16,
        disk: 20,
        disk_total: 100,
      },
      {
        online: false,
        ram: 12,
        mem_total: 32,
        disk: 80,
        disk_total: 200,
      },
    ])

    expect(summary).toEqual({
      memory: { used: 4, total: 48 },
      disk: { used: 20, total: 300 },
    })
  })
})
