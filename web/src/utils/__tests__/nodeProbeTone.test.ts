import { describe, expect, it } from 'vitest'
import { getProbeToneColor } from '../nodeHelpers'

describe('getProbeToneColor', () => {
  it('uses Emerald latency and loss boundaries', () => {
    expect([60, 100, 160, 200, 201].map(value => getProbeToneColor('latency', value))).toEqual([
      '#5EEAA6', '#47B592', '#A3E635', '#FACC15', '#F43F5E',
    ])
    expect([1, 3, 6, 9, 10].map(value => getProbeToneColor('loss', value))).toEqual([
      '#5EEAA6', '#47B592', '#A3E635', '#FACC15', '#F43F5E',
    ])
    expect(getProbeToneColor('latency', null)).toBeUndefined()
  })

  it('uses valid theme colors and ignores invalid colors', () => {
    const settings = { pingExcellentColor: '#123abc', pingCriticalColor: 'red' }
    expect(getProbeToneColor('loss', 1, settings)).toBe('#123ABC')
    expect(getProbeToneColor('loss', 10, settings)).toBe('#F43F5E')
  })
})
