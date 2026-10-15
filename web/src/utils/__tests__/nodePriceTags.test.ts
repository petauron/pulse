import type { NodeData } from '@/stores/nodes'
import { describe, expect, it } from 'vitest'
import { getPriceTags } from '../nodeHelpers'

describe('getPriceTags', () => {
  const freeNode = {
    price: 0,
    billing_cycle: 30,
    currency: 'CNY',
    expired_at: '',
  } as NodeData

  it('does not add a free-price label to the Emerald card', () => {
    expect(getPriceTags(freeNode, 'zh-CN')).toEqual([])
    const expiredAt = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString()
    const withExpiry = getPriceTags({ ...freeNode, expired_at: expiredAt }, 'zh-CN')
    expect(withExpiry).toHaveLength(1)
    expect(withExpiry[0]?.text).toMatch(/^余 \d+ 天$/)
  })

  it('keeps the existing list price label', () => {
    expect(getPriceTags(freeNode, 'zh-CN', 'list')).toEqual([{ text: '免费' }])
  })

  it('continues to show a paid card price', () => {
    expect(getPriceTags({ ...freeNode, price: 10 }, 'zh-CN')).toEqual([{ text: '¥10/月' }])
  })
})
