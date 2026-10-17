import { describe, expect, it } from 'vitest'
import { parseTags } from '../tagHelper'

describe('node tags', () => {
  it('keeps explicitly selected colors for bandwidth and route labels', () => {
    const tags = parseTags('V4<green>; CTGGIA-9929-CMI<blue>; 带宽1G<violet>')
    expect(tags.map(({ text, color }) => ({ text, color }))).toEqual([
      { text: 'V4', color: 'green' },
      { text: 'CTGGIA-9929-CMI', color: 'blue' },
      { text: '带宽1G', color: 'violet' },
    ])
  })

  it('gives plain labels a restrained default palette and skips empty labels', () => {
    const tags = parseTags(' 马来西亚 ; V4 ; ; <blue> ; 带宽1G ')
    expect(tags.map(({ text, color }) => ({ text, color }))).toEqual([
      { text: '马来西亚', color: 'gray' },
      { text: 'V4', color: 'green' },
      { text: '带宽1G', color: 'blue' },
    ])
  })
})
