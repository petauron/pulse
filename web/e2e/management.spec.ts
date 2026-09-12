import { expect, test } from '@playwright/test'
import { nodeMetadata } from '../src/utils/admin'
import { login, mutation, waitForNode } from './auth'
import { testPassword, testUsername } from './credentials.mjs'

test('private monitoring, setup lockout, administrator CSRF, and logout enforce the session boundary', async ({ page, request }, testInfo) => {
  const anonymous = await request.get('/api/auth/status')
  const status = await anonymous.json()
  await expect.poll(async () => (await (await request.get('/api/auth/status')).json()).initialized).toBe(true)
  expect(status.logged_in).toBe(false)
  expect((await request.get('/api/v1/nodes')).status()).toBe(401)
  await page.goto('/admin')
  await expect(page).toHaveURL(/\/login\?redirect=/)

  await login(page, '/admin')
  await expect(page.getByRole('heading', { name: '管理 Pulse' })).toBeVisible()
  const screenshot = testInfo.outputPath('authenticated-admin.png')
  await page.screenshot({ path: screenshot, fullPage: true })
  await testInfo.attach('authenticated-admin', { path: screenshot, contentType: 'image/png' })
  const replay = await mutation(page.request, '/api/auth/setup', { token: 'not-the-setup-token', username: testUsername, password: testPassword })
  expect(replay.ok()).toBe(false)
  const state = await (await page.request.get('/api/admin/state')).json()
  const rejected = await page.request.post('/api/admin/settings', { headers: { Origin: 'http://127.0.0.1:18080' }, data: state.settings })
  expect(rejected.status()).toBe(403)
  await page.getByRole('button', { name: '退出登录' }).click()
  await expect(page).toHaveURL('/login')
  expect((await page.request.get('/api/admin/state')).status()).toBe(401)
  await expect(page.getByText('e2e-node', { exact: true })).toHaveCount(0)
})

test('public mode still hides hidden nodes and never exposes administrator writes', async ({ page, request }) => {
  await login(page, '/admin?section=site')
  const id = await waitForNode(page.request)
  const original = await (await page.request.get('/api/admin/state')).json()
  try {
    await page.getByLabel('私有站点：登录后才能查看监控数据').uncheck()
    await page.getByRole('button', { name: '保存设置' }).click()
    await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
    expect((await request.get('/api/v1/nodes')).status()).toBe(200)

    await page.goto(`/admin?section=nodes&node=${id}`)
    await page.getByLabel('从监控视图隐藏（管理列表保留）').check()
    await page.getByRole('button', { name: '保存节点' }).click()
    await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
    const dashboard = await request.post('/api/rpc2', { data: { jsonrpc: '2.0', id: 1, method: 'common:getDashboard', params: {} } })
    expect(dashboard.ok()).toBe(true)
    expect((await dashboard.json()).result.clients).not.toHaveProperty(id)
    expect((await request.get(`/api/v1/nodes/${id}/probes?hours=24`)).status()).toBe(404)
    expect((await request.get('/api/admin/state')).status()).toBe(401)
    expect((await mutation(request, '/api/admin/settings', original.settings)).status()).toBe(401)
    await expect(page.getByText('此节点已从监控页面和探测历史中隐藏。取消隐藏并保存后可查看。')).toBeVisible()
  }
  finally {
    const node = nodeMetadata(original.nodes.find((item: { id: string }) => item.id === id))
    await mutation(page.request, `/api/admin/nodes/${id}`, node)
    await mutation(page.request, '/api/admin/settings', original.settings)
  }
})

test('probe CRUD executes on the local Agent and exposes real result history', async ({ page }, testInfo) => {
  test.setTimeout(90_000)
  await login(page, '/admin?section=probes')
  const id = await waitForNode(page.request)
  await page.getByRole('button', { name: '刷新', exact: true }).first().click()
  const name = `e2e-health-${testInfo.project.name}`
  let taskId = ''
  try {
    await page.getByLabel('名称', { exact: true }).fill(name)
    await page.getByLabel('类型', { exact: true }).selectOption('http')
    await page.getByLabel('目标', { exact: true }).fill('http://127.0.0.1:18080/healthz')
    await page.getByLabel('间隔（秒）', { exact: true }).fill('5')
    await page.getByLabel('超时（秒）', { exact: true }).fill('2')
    await page.getByLabel('e2e-node', { exact: true }).check()
    await page.getByRole('button', { name: '保存任务' }).click()
    await expect(page.getByText(name, { exact: true }).first()).toBeVisible()
    const state = await (await page.request.get('/api/admin/state')).json()
    taskId = state.probes.find((task: { name: string }) => task.name === name).id
    await expect.poll(async () => {
      const response = await page.request.get(`/api/v1/nodes/${id}/probes?hours=1`)
      return (await response.json()).records.some((record: { task_id: string, success: boolean, latency_ms: number }) => record.task_id === taskId && record.success && record.latency_ms >= 0)
    }, { timeout: 60_000 }).toBe(true)
    await page.goto(`/instance/${id}`)
    await expect(page.getByRole('table', { name: '所选时间范围的延迟与丢包汇总' })).toContainText(name)
    await expect(page.getByRole('table', { name: '所选时间范围的延迟与丢包汇总' })).toContainText('ms')
    await page.goto('/admin?section=probes')
    const task = page.getByRole('listitem').filter({ hasText: name })
    await task.getByRole('button', { name: '编辑' }).click()
    await page.getByLabel('名称', { exact: true }).fill(`${name}-edited`)
    await page.getByRole('button', { name: '保存任务' }).click()
    await page.getByRole('listitem').filter({ hasText: `${name}-edited` }).getByRole('button', { name: '删除' }).click()
    await page.getByRole('dialog').getByRole('button', { name: '确认删除' }).click()
    await expect(page.getByRole('listitem').filter({ hasText: `${name}-edited` })).toHaveCount(0)
    taskId = ''
  }
  finally {
    if (taskId)
      await mutation(page.request, `/api/admin/probes/${taskId}/delete`)
  }
})

test('notification channels require an explicit opt-in and remain disabled when created', async ({ page }, testInfo) => {
  await login(page, '/admin?section=alerts')
  const name = `e2e-disabled-${testInfo.project.name}`
  let channelId = ''
  try {
    const enabled = page.getByLabel('启用渠道并允许发送上述告警信息')
    await expect(enabled).not.toBeChecked()
    await page.getByLabel('渠道名称', { exact: true }).fill(name)
    await page.getByLabel('Webhook URL', { exact: true }).fill('https://example.invalid/pulse-e2e')
    await page.getByRole('button', { name: '保存渠道' }).click()
    await expect(page.getByRole('listitem').filter({ hasText: name })).toBeVisible()
    const state = await (await page.request.get('/api/admin/state')).json()
    const channel = state.channels.find((item: { name: string }) => item.name === name)
    channelId = channel.id
    expect(channel.enabled).toBe(false)
    await page.getByRole('listitem').filter({ hasText: name }).getByRole('button', { name: '删除' }).click()
    await page.getByRole('dialog').getByRole('button', { name: '确认删除' }).click()
    await expect(page.getByRole('listitem').filter({ hasText: name })).toHaveCount(0)
    channelId = ''
  }
  finally {
    if (channelId)
      await mutation(page.request, `/api/admin/channels/${channelId}/delete`)
  }
})
