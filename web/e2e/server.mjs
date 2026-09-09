import { spawn, spawnSync } from 'node:child_process'
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const webDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repository = resolve(webDirectory, '..')
const binarySuffix = process.platform === 'win32' ? '.exe' : ''
const serviceBinary = join(repository, 'target', 'debug', `pulse-service${binarySuffix}`)
const agentBinary = join(repository, 'target', 'debug', `pulse-agent${binarySuffix}`)
const stateDirectory = mkdtempSync(join(tmpdir(), 'pulse-e2e-'))
const databasePath = join(stateDirectory, 'pulse.db')
const tokenPath = join(stateDirectory, 'enrollment-token')
const credentialsPath = join(stateDirectory, 'agent-credentials.json')
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
    },
    stdio: 'inherit',
  }))
}, 500)

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
