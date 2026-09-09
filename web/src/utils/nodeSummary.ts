import type { NodeData } from '@/stores/nodes'

type CapacityNode = Pick<NodeData, 'online' | 'ram' | 'mem_total' | 'disk' | 'disk_total'>

export interface CapacitySummary {
  memory: {
    used: number
    total: number
  }
  disk: {
    used: number
    total: number
  }
}

/**
 * Summarize current resource use without treating an offline node's retained
 * last sample as live data. Installed capacity remains a fleet-wide total.
 */
export function summarizeNodeCapacity(nodes: CapacityNode[]): CapacitySummary {
  let memoryUsed = 0
  let memoryTotal = 0
  let diskUsed = 0
  let diskTotal = 0

  for (const node of nodes) {
    memoryTotal += node.mem_total || 0
    diskTotal += node.disk_total || 0

    if (node.online) {
      memoryUsed += node.ram || 0
      diskUsed += node.disk || 0
    }
  }

  return {
    memory: { used: memoryUsed, total: memoryTotal },
    disk: { used: diskUsed, total: diskTotal },
  }
}
