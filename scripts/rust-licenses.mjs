// cargo-about resolves the locked Linux dependency graph and SPDX expressions.
// Its selected license texts can omit standalone copyright/NOTICE files or
// sections of composite licenses, so preserve the packages' originals as well.
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const cargoAbout = process.env.CARGO_ABOUT || 'cargo-about'
const check = process.argv.slice(2).includes('--check')
const temporary = mkdtempSync(join(tmpdir(), 'pulse-license-output-'))
const read = path => readFileSync(path, 'utf8')
const run = (command, args) => execFileSync(command, args, {
  cwd: repository,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
  stdio: ['ignore', 'pipe', 'inherit'],
})
const escapeHtml = text => text.replace(/[&<>"']/g, character => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
})[character])

function licenseFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory())
      return licenseFiles(path)
    return entry.isFile() && /^(?:licen[cs]es?|notices?|copying|copyright)(?:[._-]|$)/i.test(entry.name)
      ? [path]
      : []
  }).sort()
}

function output(name, contents) {
  const path = join(repository, name)
  if (check) {
    if (read(path) !== contents)
      throw new Error(`${name} is stale; regenerate and review the Rust license bundle`)
  }
  else {
    writeFileSync(path, contents)
  }
}

try {
  if (run(cargoAbout, ['--version']).trim() !== 'cargo-about 0.9.2')
    throw new Error('Rust license generation requires cargo-about 0.9.2')
  const options = ['--workspace', '--all-features', '--locked', '--offline', '--fail']
  const htmlPath = join(temporary, 'licenses.html')
  run(cargoAbout, ['generate', 'about.hbs', '--output-file', htmlPath, ...options])
  const data = JSON.parse(run(cargoAbout, ['generate', '--format', 'json', ...options]))
  const packages = data.crates.map(crate => crate.package)
    .filter(crate => crate.source !== null)
    .sort((a, b) => `${a.name}@${a.version}` < `${b.name}@${b.version}` ? -1 : 1)
  const sections = ['<h2>Original package license and notice files</h2>']
  for (const crate of packages) {
    const root = dirname(crate.manifest_path)
    const paths = new Set(licenseFiles(root))
    if (crate.license_file)
      paths.add(resolve(root, crate.license_file))
    if (paths.size === 0)
      throw new Error(`${crate.name} ${crate.version} has no packaged license files; review and clarify before release`)
    sections.push(`<h3>${escapeHtml(crate.name)} ${escapeHtml(crate.version)}</h3>`)
    for (const path of [...paths].sort()) {
      sections.push(`<h4>${escapeHtml(relative(root, path))}</h4>`)
      sections.push(`<pre>${escapeHtml(read(path))}</pre>`)
    }
  }
  const marker = '<!-- ORIGINAL_CRATE_LICENSES -->'
  const html = read(htmlPath)
  if (!html.includes(marker))
    throw new Error('License template is missing the original-notice appendix marker')
  output('RUST_THIRD_PARTY_LICENSES.html', html.replace(marker, sections.join('\n')))

  const sysroot = run('rustc', ['--print', 'sysroot']).trim()
  output('RUST_STDLIB_LICENSES.html', read(join(sysroot, 'share/doc/rust/COPYRIGHT-library.html')))
  console.log(`Rust licenses ${check ? 'verified' : 'generated'} for ${packages.length} Linux dependency packages and the pinned standard library`)
}
finally {
  rmSync(temporary, { recursive: true, force: true })
}
