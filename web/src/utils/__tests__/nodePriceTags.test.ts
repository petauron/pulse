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

  it('shows free and long-term labels as the deployed Komari card does', () => {
    expect(getPriceTags(freeNode, 'zh-CN')).toEqual([{ text: '免费' }])
    expect(getPriceTags({ ...freeNode, expired_at: '9999-01-01T00:00:00Z' }, 'zh-CN'))
      .toEqual([{ text: '免费' }, { text: '长期' }])
  })

  it('keeps the existing list price label', () => {
    expect(getPriceTags(freeNode, 'zh-CN', 'list')).toEqual([{ text: '免费' }])
  })

  it('continues to show a paid card price', () => {
    expect(getPriceTags({ ...freeNode, price: 10 }, 'zh-CN')).toEqual([{ text: '¥10/月' }])
  })
})
