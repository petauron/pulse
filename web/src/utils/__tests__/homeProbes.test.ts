import type { NodeProbe } from '../rpc'
import { describe, expect, it } from 'vitest'
import { selectHomeProbes } from '../nodeHelpers'

function probe(id: string): NodeProbe {
  return { id, name: id, kind: 'tcp', interval_seconds: 60, points: [] }
}

describe('selectHomeProbes', () => {
  const probes = [probe('a'), probe('b'), probe('c')]

  it('uses Emerald default of two rows and keeps empty slots visible', () => {
    expect(selectHomeProbes(probes, 'node')).toEqual([probes[0], probes[1]])
    expect(selectHomeProbes([], 'node')).toEqual([null, null])
  })

  it('follows per-node task order and fills missing rows', () => {
    const settings = { homePingRowCount: 3, homePingTasksByNode: { node: ['c', 'missing', 'a'] } }
    expect(selectHomeProbes(probes, 'node', settings)).toEqual([probes[2], null, probes[0]])
    expect(selectHomeProbes(probes, 'other', settings)).toEqual(probes)
    expect(selectHomeProbes([probe('7')], 'node', { homePingRowCount: 1, homePingTasksByNode: { node: [7] } })).toEqual([probe('7')])
  })

  it('bounds row count and preserves an explicit empty selection', () => {
    expect(selectHomeProbes(probes, 'node', { homePingRowCount: 0 })).toEqual([probes[0]])
    expect(selectHomeProbes(probes, 'node', { homePingRowCount: 3, homePingTasksByNode: { node: [] } })).toEqual([null, null, null])
    expect(selectHomeProbes([], 'node', { homePingRowCount: 99 })).toHaveLength(8)
  })
})
