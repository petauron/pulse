import { readFileSync } from 'node:fs'

const generatedIcons = readFileSync(new URL('../src/generated/icons.ts', import.meta.url), 'utf8')
const notices = readFileSync(new URL('../THIRD_PARTY_NOTICES.md', import.meta.url), 'utf8')
const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))
const lucideLicense = readFileSync(new URL('../LICENSE.lucide', import.meta.url), 'utf8')
const tablerLicense = readFileSync(new URL('../LICENSE.tabler', import.meta.url), 'utf8')
const echartsD3License = readFileSync(new URL('../LICENSE.echarts-d3', import.meta.url), 'utf8')
const bundledLicenses = readFileSync(new URL('../dist/THIRD_PARTY_LICENSES.md', import.meta.url), 'utf8')
const echartsPackage = JSON.parse(readFileSync(new URL('../node_modules/echarts/package.json', import.meta.url), 'utf8'))

const requiredCollections = [
  ['icon-park-outline', 'IconPark Outline 1.4.2', 'Apache-2.0'],
  ['lucide', 'Lucide', 'ISC'],
  ['tabler', 'Tabler Icons 3.46.0', 'MIT'],
]

for (const [prefix, heading, license] of requiredCollections) {
  if (!generatedIcons.includes(`"prefix": "${prefix}"`))
    throw new Error(`Generated icon collection is missing: ${prefix}`)
  if (!notices.includes(heading) || !notices.includes(license))
    throw new Error(`Third-party notice is incomplete for: ${prefix}`)
}

if (packageJson.license !== 'Apache-2.0' || packageJson.private !== true)
  throw new Error('Pulse Web package metadata must identify private Apache-2.0 work')

if (!lucideLicense.includes('ISC License') || !lucideLicense.includes('Cole Bemis'))
  throw new Error('Lucide and Feather license terms are incomplete')
if (!tablerLicense.includes('MIT License') || !tablerLicense.includes('Paweł Kuna'))
  throw new Error('Tabler license terms are incomplete')
if (!notices.includes(`Apache ECharts ${echartsPackage.version}`) || !notices.includes('The Apache Software Foundation'))
  throw new Error('Apache ECharts NOTICE terms are incomplete')
if (!echartsD3License.includes('Copyright 2010-2016 Mike Bostock'))
  throw new Error('ECharts D3-derived license terms are incomplete')

for (const dependency of ['@iconify/vue', 'cobe', 'dayjs', 'echarts', 'pinia', 'vue', 'vue-echarts']) {
  if (!bundledLicenses.includes(dependency))
    throw new Error(`Bundled dependency license is missing: ${dependency}`)
}

const allowedBundledLicenses = new Set(['0BSD', 'Apache-2.0', 'BSD-3-Clause', 'ISC', 'MIT', 'Zlib'])
const bundledHeadings = [...bundledLicenses.matchAll(/^## .+ \(([^()]+)\)$/gm)]
if (bundledHeadings.length === 0)
  throw new Error('Bundled dependency license inventory is empty')
for (const [, license] of bundledHeadings) {
  if (!allowedBundledLicenses.has(license))
    throw new Error(`Bundled dependency uses an unreviewed license: ${license}`)
}

console.log('Third-party license notices are complete.')
