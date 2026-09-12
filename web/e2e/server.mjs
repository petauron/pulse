import { spawn, spawnSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { testPassword, testUsername } from './credentials.mjs'

const webDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repository = resolve(webDirectory, '..')
const binarySuffix = process.platform === 'win32' ? '.exe' : ''
const serviceBinary = join(repository, 'target', 'debug', `pulse-service${binarySuffix}`)
const agentBinary = join(repository, 'target', 'debug', `pulse-agent${binarySuffix}`)
const stateDirectory = mkdtempSync(join(tmpdir(), 'pulse-e2e-'))
const databasePath = join(stateDirectory, 'pulse.db')
const tokenPath = join(stateDirectory, 'enrollment-token')
const credentialsPath = join(stateDirectory, 'agent-credentials.json')
const setupTokenPath = join(stateDirectory, 'setup-token')
const setupToken = randomBytes(32).toString('hex')
writeFileSync(setupTokenPath, `${setupToken}\n`, { mode: 0o600 })
const serviceUrl = 'http://127.0.0.1:18080/'
const baseEnvironment = {
  ...process.env,
  PATH: `${dirname(serviceBinary)}${delimiter}${process.env.PATH ?? ''}`,
  PULSE_DATABASE_PATH: databasePath,
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
      PULSE_LISTEN: '127.0.0.1:18080',
      PULSE_PUBLIC_URL: 'http://127.0.0.1:18080',
      PULSE_SETUP_TOKEN_FILE: setupTokenPath,
      PULSE_RETENTION_DAYS: '7',
      PULSE_OFFLINE_AFTER_SECONDS: '20',
      PULSE_MAX_NODES: '10',
      PULSE_MAX_DATABASE_BYTES: '67108864',
    },
    stdio: 'inherit',
  }),
]

const agentTimer = setTimeout(() => {
  children.push(spawn(agentBinary, [], {
    cwd: repository,
    env: {
      ...process.env,
      PULSE_SERVICE_URL: serviceUrl,
      PULSE_ENROLLMENT_TOKEN_FILE: tokenPath,
      PULSE_CREDENTIALS_PATH: credentialsPath,
      PULSE_NODE_NAME: 'e2e-node',
      PULSE_NODE_REGION: 'SG',
      PULSE_NODE_GROUP: 'e2e',
      PULSE_INTERVAL_SECONDS: '5',
      PULSE_GEOIP_PROVIDER: 'disabled',
    },
    stdio: 'inherit',
  }))
}, 500)

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
        body: JSON.stringify({ token: setupToken, username: testUsername, password: testPassword }),
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

initializeAdministrator().catch(() => {
  process.stderr.write('Failed to initialize the disposable E2E administrator\n')
  cleanup()
})

let closing = false
function cleanup(signal = 'SIGTERM') {
  if (closing)
    return
  closing = true
  clearTimeout(agentTimer)
  for (const child of children) {
    if (!child.killed)
      child.kill(signal)
  }
  rmSync(stateDirectory, { recursive: true, force: true })
  process.exit(0)
}

process.on('SIGINT', () => cleanup('SIGINT'))
process.on('SIGTERM', () => cleanup('SIGTERM'))
for (const child of children) {
  child.on('exit', (code, signal) => {
    if (!closing && code !== 0) {
      process.stderr.write(`E2E child exited unexpectedly: code=${code} signal=${signal}\n`)
      cleanup()
    }
  })
}

setInterval(() => {}, 60_000)
