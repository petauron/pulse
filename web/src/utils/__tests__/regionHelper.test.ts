import { readdirSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { emojiToRegionMap, getFlagSrc } from '../regionHelper'

describe('getFlagSrc', () => {
  it.each(['SG', 'sg', ' SG ', '🇸🇬'])('uses the lowercase packaged flag for %s', (region) => {
    expect(getFlagSrc(region)).toBe('/assets/flags/sg.svg')
  })

  it('resolves every supported region to an exact packaged filename', () => {
    // Compare directory entries so this also catches case errors on macOS.
    const files = new Set(readdirSync(new URL('../../../node_modules/flag-icons/flags/4x3/', import.meta.url)))
    for (const [emoji, { code }] of Object.entries(emojiToRegionMap)) {
      const filename = getFlagSrc(emoji).slice('/assets/flags/'.length)
      expect(files.has(filename), `${code}: ${filename}`).toBe(true)
      expect(getFlagSrc(code)).toBe(getFlagSrc(emoji))
    }
  })

  it.each(['', 'ZZ', '../../api/public', 'unknown'])('uses the packaged placeholder for %s', (region) => {
    expect(getFlagSrc(region)).toBe('/assets/flags/xx.svg')
  })
})
