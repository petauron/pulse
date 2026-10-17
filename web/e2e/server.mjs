import { spawn, spawnSync } from 'node:child_process'
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { testPassword, testUsername } from './credentials.mjs'

const webDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repository = resolve(webDirectory, '..')
const binarySuffix = process.platform === 'win32' ? '.exe' : ''
const serviceBinary = join(repository, 'target', 'debug', `senulun-service${binarySuffix}`)
const agentBinary = join(repository, 'target', 'debug', `senulun-agent${binarySuffix}`)
const stateDirectory = mkdtempSync(join(tmpdir(), 'senulun-e2e-'))
const databasePath = join(stateDirectory, 'senulun.db')
const tokenPath = join(stateDirectory, 'enrollment-token')
const credentialsPath = join(stateDirectory, 'agent-credentials.json')
const serviceUrl = 'http://127.0.0.1:18080/'
const baseEnvironment = {
  ...process.env,
  PATH: `${dirname(serviceBinary)}${delimiter}${process.env.PATH ?? ''}`,
  SENULUN_DATABASE_PATH: databasePath,
}

const enrollment = spawnSync(
  serviceBinary,
  ['enrollment', 'create', '--ttl-seconds', '600'],
  { cwd: repository, env: baseEnvironment, encoding: 'utf8' },
)
if (enrollment.status !== 0) {
  process.stderr.write(enrollment.stderr)
  throw new Error('failed to create the E2E enrollment token')
}
const token = JSON.parse(enrollment.stdout).token
writeFileSync(tokenPath, `${token}\n`, { mode: 0o600 })
chmodSync(stateDirectory, 0o700)

const children = [
  spawn(serviceBinary, ['serve'], {
    cwd: repository,
    env: {
      ...baseEnvironment,
      SENULUN_LISTEN: '127.0.0.1:18080',
      SENULUN_PUBLIC_URL: 'http://127.0.0.1:18080',
      SENULUN_RETENTION_DAYS: '7',
      SENULUN_OFFLINE_AFTER_SECONDS: '20',
      SENULUN_MAX_NODES: '10',
      SENULUN_MAX_DATABASE_BYTES: '67108864',
    },
    stdio: 'inherit',
  }),
]

function startAgent() {
  const child = spawn(agentBinary, [], {
    cwd: repository,
    env: {
      ...process.env,
      SENULUN_SERVICE_URL: serviceUrl,
      SENULUN_ENROLLMENT_TOKEN_FILE: tokenPath,
      SENULUN_CREDENTIALS_PATH: credentialsPath,
      SENULUN_NODE_NAME: 'e2e-node',
      SENULUN_NODE_REGION: 'SG',
      SENULUN_NODE_GROUP: 'e2e',
      SENULUN_INTERVAL_SECONDS: '5',
      SENULUN_GEOIP_PROVIDER: 'disabled',
    },
    stdio: 'inherit',
  })
  children.push(child)
  observeChild(child)
}

async function initializeAdministrator() {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(new URL('api/auth/status', serviceUrl))
      if (!response.ok)
        throw new Error('authentication status is not ready')
      const status = await response.json()
      if (status.initialized)
        throw new Error('disposable E2E database was unexpectedly initialized')
      const cookie = response.headers.getSetCookie().map(value => value.split(';')[0]).join('; ')
      const setup = await fetch(new URL('api/auth/setup', serviceUrl), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Origin': 'http://127.0.0.1:18080', 'Cookie': cookie, 'X-CSRF-Token': status.csrf_token },
        body: JSON.stringify({ username: testUsername, password: testPassword }),
      })
      if (!setup.ok)
        throw new Error(`E2E administrator setup failed with HTTP ${setup.status}`)
      const initialized = await setup.json()
      if (!initialized.initialized || !initialized.logged_in || !initialized.csrf_token)
        throw new Error('E2E setup did not return a complete session')
      return
    }
    catch (error) {
      if (attempt === 99)
        throw error
      await new Promise(resolve => setTimeout(resolve, 100))
    }
  }
}

// Initial setup and enrollment both write the disposable database. Start the
// Agent only after setup commits instead of racing a fixed startup delay.
initializeAdministrator().then(startAgent).catch(() => {
  process.stderr.write('Failed to initialize the disposable E2E administrator\n')
  cleanup('SIGTERM', 1)
})

let closing = false
function cleanup(signal = 'SIGTERM', exitCode = 0) {
  if (closing)
    return
  closing = true
  for (const child of children) {
    if (!child.killed)
      child.kill(signal)
  }
  rmSync(stateDirectory, { recursive: true, force: true })
  process.exit(exitCode)
}

process.on('SIGINT', () => cleanup('SIGINT'))
process.on('SIGTERM', () => cleanup('SIGTERM'))
function observeChild(child) {
  child.on('exit', (code, signal) => {
    if (!closing) {
      process.stderr.write(`E2E child exited unexpectedly: code=${code} signal=${signal}\n`)
      cleanup('SIGTERM', 1)
    }
  })
}
for (const child of children)
  observeChild(child)

setInterval(() => {}, 60_000)
