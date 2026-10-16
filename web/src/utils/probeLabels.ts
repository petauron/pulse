export type ProbeKind = 'icmp' | 'tcp' | 'http'

export function failureRateLabel(kind: ProbeKind, short = false): string {
  // Emerald calls unsuccessful ping checks "loss" for both ICMP and TCP.
  return kind === 'http' ? (short ? '失败' : '失败率') : (short ? '丢包' : '丢包率')
}

export function failureRateLabelForKinds(kinds: readonly ProbeKind[], short = false): string {
  const hasPing = kinds.some(kind => kind !== 'http')
  const hasHttp = kinds.includes('http')
  if (hasPing && !hasHttp)
    return failureRateLabel('tcp', short)
  if (hasHttp && !hasPing)
    return failureRateLabel('http', short)
  return short ? '丢包 / 失败' : '丢包率 / 失败率'
}
