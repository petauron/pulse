import { getSharedApi } from './api'

export interface SiteSettings {
  site_name: string
  private_site: boolean
  agent_interval_seconds: number
}

export interface ManagedNode {
  id: string
  name: string
  region: string
  group: string
  weight: number
  hidden: boolean
  tags: string
  public_remark: string
  price: number
  currency: string
  billing_cycle_days: number
  expired_at_unix_ms: number | null
  auto_renewal: boolean
  traffic_limit_bytes: number
  traffic_limit_type: 'sum' | 'max' | 'min' | 'up' | 'down'
  traffic_reset_day: number
  disabled?: boolean
}

export interface ProbeTask {
  id: string
  name: string
  kind: 'icmp' | 'tcp' | 'http'
  target: string
  interval_seconds: number
  timeout_seconds: number
  enabled: boolean
  node_ids: string[]
}

export interface NotificationChannel {
  id: string
  name: string
  kind: 'webhook'
  url: string
  enabled: boolean
}

export interface AlertRule {
  id: string
  name: string
  node_ids: string[]
  metric: 'offline' | 'cpu' | 'memory' | 'disk' | 'traffic' | 'expiry'
  threshold: number
  duration_seconds: number
  cooldown_seconds: number
  channel_ids: string[]
  enabled: boolean
}

export interface Incident {
  id: string
  node_id: string
  rule_id: string
  message: string
  status: string
  opened_at_unix_ms: number
  updated_at_unix_ms: number
}

export interface AdminState {
  settings: SiteSettings
  nodes: ManagedNode[]
  probes: ProbeTask[]
  alert_rules: AlertRule[]
  channels: NotificationChannel[]
  incidents: Incident[]
  notification_failures: { happened_at_unix_ms: number, subject: string }[]
}

export interface ProbeRecord {
  task_id: string
  collected_at_unix_ms: number
  received_at_unix_ms: number
  latency_ms: number | null
  success: boolean
  error: string | null
}

export interface ProbeHistory {
  tasks: Pick<ProbeTask, 'id' | 'name' | 'kind' | 'interval_seconds'>[]
  records: ProbeRecord[]
  summary: { task_id: string, samples: number, loss_percent: number, avg_latency_ms: number | null }[]
  limit: number
  history_order: 'newest_first'
}

/** Do not send read-only counters or newly added admin-state fields as metadata. */
export function nodeMetadata(node: ManagedNode): ManagedNode {
  const { id, name, region, group, weight, hidden, tags, public_remark, price, currency, billing_cycle_days, expired_at_unix_ms, auto_renewal, traffic_limit_bytes, traffic_limit_type, traffic_reset_day } = node
  return { id, name, region, group, weight, hidden, tags, public_remark, price, currency, billing_cycle_days, expired_at_unix_ms, auto_renewal, traffic_limit_bytes, traffic_limit_type, traffic_reset_day }
}

export function fetchProbeHistory(id: string, hours: number, signal?: AbortSignal): Promise<ProbeHistory> {
  return getSharedApi().get(`v1/nodes/${encodeURIComponent(id)}/probes?hours=${hours}`, signal)
}
