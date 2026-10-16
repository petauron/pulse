export type ProbeKind = 'icmp' | 'tcp' | 'http'

export function failureRateLabel(kind: ProbeKind, short = false): string {
  return kind === 'icmp' ? (short ? '丢包' : '丢包率') : (short ? '失败' : '失败率')
}

export function failureRateLabelForKinds(kinds: readonly ProbeKind[], short = false): string {
  const hasIcmp = kinds.includes('icmp')
  const hasOther = kinds.some(kind => kind !== 'icmp')
  if (hasIcmp && !hasOther)
    return failureRateLabel('icmp', short)
  if (hasOther && !hasIcmp)
    return failureRateLabel('tcp', short)
  return short ? '丢包 / 失败' : '丢包率 / 失败率'
}
