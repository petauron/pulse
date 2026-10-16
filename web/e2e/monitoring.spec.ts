import { Buffer } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'
import { login } from './auth'

test('real Agent is visible through the Emerald card, list, search, detail, and history flow', async ({ page }, testInfo) => {
  const consoleErrors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error')
      consoleErrors.push(message.text())
  })

  await login(page)
  await expect(page.getByRole('button', { name: '访问者信息' })).toBeVisible()
  const card = page.getByRole('button', { name: '查看 e2e-node 节点详情' })
  await expect(card).toBeVisible({ timeout: 20_000 })
  await expect(page.getByTestId('node-card-grid')).toHaveClass(/auto-rows-fr/)
  await expect(card).toContainText('e2e-node')
  await expect(card.getByLabel(/^最近 .+的平均延迟与丢包（TCP\/HTTP 为探测失败率），每格约 \d+ 分钟$/)).toBeVisible()
  await expect(card.locator('.grid.grid-cols-2 > div').nth(3).locator('.truncate').first()).toHaveCSS('white-space', 'nowrap')
  const flag = card.getByRole('img', { name: 'SG', exact: true })
  await expect(flag).toHaveAttribute('src', '/assets/flags/sg.svg')
  await expect.poll(() => flag.evaluate(image => (image as HTMLImageElement).naturalWidth)).toBeGreaterThan(0)
  const cardScreenshot = testInfo.outputPath('node-card.png')
  await page.screenshot({ path: cardScreenshot, fullPage: true })
  await testInfo.attach('node-card', { path: cardScreenshot, contentType: 'image/png' })

  const valueTrigger = page.getByRole('button', { name: '查看剩余价值详情' })
  await valueTrigger.focus()
  await valueTrigger.press('Enter')
  await expect(valueTrigger).toHaveAttribute('aria-expanded', 'true')
  const valuePanel = page.getByRole('region', { name: '剩余价值详情' })
  await expect(valuePanel).toBeVisible()
  await expect(valuePanel).toHaveCSS('height', '168px')
  await expect(page.getByRole('button', { name: '关闭价值详情' })).toBeFocused()
  await expect(page.getByText('月均支出', { exact: true })).toBeVisible()
  await page.getByRole('combobox', { name: '切换汇率基准币种' }).selectOption('USD')
  await expect(valueTrigger).toContainText('USD')
  await page.getByRole('button', { name: '查看汇率来源与估算说明' }).focus()
  await expect(page.getByRole('tooltip')).toContainText('月均按 30 天折算')
  await page.keyboard.press('Escape')
  await expect(valueTrigger).toHaveAttribute('aria-expanded', 'false')
  await expect(valueTrigger).toBeFocused()
  await page.reload()
  await expect(valueTrigger).toContainText('USD')

  await page.getByRole('button', { name: '列表视图' }).click()
  const row = page.getByRole('button', { name: /查看 e2e-node 节点详情，当前在线/ })
  await expect(row).toBeVisible()
  await page.getByRole('button', { name: /^CPU/ }).click()
  await expect(page.getByRole('columnheader', { name: /CPU/ })).toHaveAttribute('aria-sort', 'ascending')

  const search = page.getByRole('textbox', { name: '搜索节点' })
  await search.fill('missing-node')
  await expect(page.getByText('暂无节点')).toBeVisible()
  await search.fill('e2e-node')
  await expect(row).toBeVisible()

  await row.press('Enter')
  await expect(page).toHaveURL(/\/instance\/[0-9a-f-]+$/)
  await expect(page.getByRole('button', { name: '访问者信息' })).toBeVisible()
  await expect(page.getByText('节点价格', { exact: true }).locator('../..')).toContainText(/\$[\d,.]+\s*USD/)
  await expect(page.getByText('剩余价值', { exact: true }).locator('../..')).toContainText(/\$[\d,.]+\s*USD/)
  await expect(page.getByText('e2e-node', { exact: true })).toBeVisible()
  await expect(page.getByText('在线', { exact: true })).toBeVisible()
  await expect(page.getByText('CPU', { exact: true }).first()).toBeVisible()
  await expect(page.getByRole('tab', { name: '4 小时', exact: true })).toBeVisible()
  await expect(page.getByText(/查看最近 .* 个采样点的数据表/)).toBeVisible({ timeout: 20_000 })

  const accessibility = await new AxeBuilder({ page }).analyze()
  const seriousViolations = accessibility.violations.filter(({ impact }) =>
    impact === 'serious' || impact === 'critical',
  )
  expect(seriousViolations).toEqual([])
  expect(consoleErrors).toEqual([])
  const detailScreenshot = testInfo.outputPath('node-detail.png')
  await page.screenshot({ path: detailScreenshot, fullPage: true })
  await testInfo.attach('node-detail', { path: detailScreenshot, contentType: 'image/png' })

  await page.getByRole('button', { name: '返回节点列表' }).click()
  await expect(page).toHaveURL('/')
})

test('the first failed connection recovers without reloading the page', async ({ page }) => {
  await login(page, '/admin')
  await expect(page.getByRole('button', { name: '访问者信息' })).toHaveCount(0)
  let failRequests = true
  await page.route('**/api/public', async (route) => {
    if (failRequests)
      await route.fulfill({ status: 503, body: 'Service temporarily unavailable' })
    else
      await route.continue()
  })
  await page.goto('/')
  await expect(page.getByText('连接服务器失败，正在自动重试。请检查网络设置。')).toBeVisible()
  failRequests = false
  await expect(page.getByRole('button', { name: '查看 e2e-node 节点详情' })).toBeVisible({ timeout: 20_000 })
  await expect(page.getByText('连接服务器失败，正在自动重试。请检查网络设置。')).toBeHidden()
})

test('Emerald maps mode renders the bundled world map with node counts', async ({ page }) => {
  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.theme_settings = {
      ...payload.data.theme_settings,
      earthViewMode: 'maps',
    }
    await route.fulfill({ response, json: payload })
  })

  await login(page)
  const map = page.getByTestId('earth-map')
  await expect(map).toBeVisible()
  await expect(map).toHaveAttribute('aria-label', /世界地图：共 \d+ 个节点，在线 \d+ 个，离线 \d+ 个/)
  await expect(map.locator('canvas')).toBeVisible()
  const asset = await page.request.get('/maps/world.json')
  expect(asset.ok()).toBe(true)
})

test('network quality offers the full retained range with an exact custom label', async ({ page }) => {
  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.ping_record_preserve_time = 2400
    await route.fulfill({ response, json: payload })
  })
  await login(page, '/network-quality')
  await expect(page.getByRole('button', { name: '访问者信息' })).toBeVisible()
  const ranges = page.getByRole('group', { name: '统计范围' })
  await expect(ranges.getByRole('button')).toHaveText(['1 小时', '6 小时', '12 小时', '1 天', '7 天', '30 天', '60 天', '90 天', '100 天'])
  await ranges.getByRole('button', { name: '100 天' }).click()
  await expect(ranges.getByRole('button', { name: '100 天' })).toHaveAttribute('aria-pressed', 'true')

  await page.goto('/')
  await page.getByRole('button', { name: '查看 e2e-node 节点详情' }).click()
  await expect(page.getByRole('tab', { name: '4 小时', exact: true })).toBeVisible()
  await expect(page.getByRole('tab', { name: '100 天', exact: true })).toBeVisible()
})

test('probe history honors Emerald chart defaults and retained range', async ({ page }) => {
  let retentionHours = 168
  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.ping_record_preserve_time = retentionHours
    payload.data.theme_settings = {
      ...payload.data.theme_settings,
      pingChartDefaultHours: 72,
      pingChartLayout: 'split',
      pingChartAutoRefresh: false,
    }
    await route.fulfill({ response, json: payload })
  })
  const requests: string[] = []
  await page.route('**/api/v1/nodes/*/probes?*', async (route) => {
    requests.push(route.request().url())
    await route.fulfill({
      json: {
        tasks: [{ id: 'e2e-probe', name: 'e2e-probe', kind: 'tcp', interval_seconds: 60 }],
        records: [],
        chart: [],
        summary: [],
        chart_source_points: 0,
        chart_bucket_ms: 3601,
        range_start_unix_ms: Date.now() - 72 * 3_600_000,
        range_end_unix_ms: Date.now(),
      },
    })
  })
  await login(page)
  await page.getByRole('button', { name: '查看 e2e-node 节点详情' }).click()
  const ranges = page.getByRole('tablist', { name: '探测历史时间范围' })
  await expect(ranges.getByRole('tab')).toHaveText(['1 小时', '6 小时', '12 小时', '1 天', '3 天', '7 天'])
  await expect(ranges.getByRole('tab', { name: '3 天' })).toHaveAttribute('data-state', 'active')
  await expect(page.getByRole('group', { name: '图表布局' }).getByRole('button', { name: '分图' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByRole('button', { name: '自动刷新' })).toBeVisible()
  await expect.poll(() => requests.length).toBe(1)
  expect(requests[0]).toContain('hours=72')

  retentionHours = 24
  await page.reload()
  await expect(ranges.getByRole('tab')).toHaveText(['1 小时', '6 小时', '12 小时', '1 天'])
  await expect(ranges.getByRole('tab', { name: '1 小时' })).toHaveAttribute('data-state', 'active')
  await expect.poll(() => requests.length).toBe(2)
  expect(requests[1]).toContain('hours=1')
})

test('probe history does not leave stale metrics visible after refresh fails', async ({ page }) => {
  let responseMode: 'data' | 'error' | 'empty' = 'data'
  let releaseEmptyResponse: (() => void) | undefined
  await page.route('**/api/v1/nodes/*/probes?*', async (route) => {
    if (responseMode === 'error') {
      await route.fulfill({ status: 503, json: { error: '探测暂不可用' } })
      return
    }
    if (responseMode === 'empty')
      await new Promise<void>((resolve) => { releaseEmptyResponse = resolve })
    await route.fulfill({
      json: {
        tasks: responseMode === 'empty' ? [] : [{ id: 'e2e-probe', name: 'e2e-probe', kind: 'icmp', interval_seconds: 60 }],
        records: [],
        chart: [],
        summary: [],
        chart_source_points: 0,
        chart_bucket_ms: 3601,
        range_start_unix_ms: Date.now() - 3_600_000,
        range_end_unix_ms: Date.now(),
      },
    })
  })
  await login(page)
  await page.getByRole('button', { name: '查看 e2e-node 节点详情' }).click()
  await expect(page.getByRole('region', { name: '延迟数据分析' })).toBeVisible()

  responseMode = 'error'
  await page.getByRole('button', { name: '立即刷新' }).click()
  await expect(page.getByRole('alert')).toContainText('探测暂不可用')
  await expect(page.getByRole('region', { name: '延迟数据分析' })).toHaveCount(0)

  responseMode = 'empty'
  await page.getByRole('button', { name: '重新尝试' }).click()
  await expect.poll(() => releaseEmptyResponse !== undefined).toBe(true)
  await expect(page.getByRole('status').filter({ hasText: '正在读取探测记录…' })).toBeVisible()
  releaseEmptyResponse?.()
  await expect(page.getByText('暂无延迟数据')).toBeVisible()
  await expect(page.getByRole('alert')).toHaveCount(0)
})

test('probe analysis switches tasks without changing the selected chart series', async ({ page }, testInfo) => {
  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.theme_settings = {
      ...payload.data.theme_settings,
      pingTaskColors: { 'e2e-probe': '#123456', 'e2e-probe-2': 'invalid' },
    }
    await route.fulfill({ response, json: payload })
  })
  await page.route('**/api/v1/nodes/*/probes?*', async (route) => {
    await route.fulfill({
      json: {
        tasks: [
          { id: 'e2e-probe', name: 'e2e-probe', kind: 'tcp', interval_seconds: 60 },
          { id: 'e2e-probe-2', name: 'e2e-probe-2', kind: 'icmp', interval_seconds: 30 },
        ],
        records: [{ task_id: 'e2e-probe', collected_at_unix_ms: Date.now(), received_at_unix_ms: Date.now(), latency_ms: 20, success: true, error: null }],
        chart: [
          { task_id: 'e2e-probe', received_at_unix_ms: Date.now(), latency_ms: 20, failed_samples: 0, samples: 1 },
          { task_id: 'e2e-probe-2', received_at_unix_ms: Date.now(), latency_ms: 40, failed_samples: 1, samples: 2 },
        ],
        summary: [
          { task_id: 'e2e-probe', samples: 4, loss_percent: 25, avg_latency_ms: 20, min_latency_ms: 10, max_latency_ms: 30, latest_latency_ms: 20, p50_latency_ms: 20, p95_latency_ms: 27, p99_latency_ms: 29, p99_p50_ratio: 0.45 },
          { task_id: 'e2e-probe-2', samples: 2, loss_percent: 50, avg_latency_ms: 40, min_latency_ms: 40, max_latency_ms: 40, latest_latency_ms: 40, p50_latency_ms: 40, p95_latency_ms: 40, p99_latency_ms: 40, p99_p50_ratio: 0 },
        ],
        chart_source_points: 3,
        chart_bucket_ms: 3601,
        range_start_unix_ms: Date.now() - 3_600_000,
        range_end_unix_ms: Date.now(),
      },
    })
  })
  await login(page)
  await page.getByRole('button', { name: '查看 e2e-node 节点详情' }).click()
  const analysis = page.getByRole('region', { name: '延迟数据分析' })
  const taskColors = analysis.locator('span[style*="background-color"]')
  await expect(taskColors.nth(0)).toHaveCSS('background-color', 'rgb(18, 52, 86)')
  await expect(taskColors.nth(1)).toHaveCSS('background-color', 'rgb(78, 205, 196)')
  await expect(analysis.getByRole('button', { name: '分析 e2e-probe', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await expect(analysis).toContainText('27 ms')
  await expect(analysis).toContainText('P99')
  await expect(analysis).toContainText('0.45')
  await expect(analysis.locator('[title="TCP/HTTP 探测失败率"]')).toHaveCount(2)
  const series = analysis.getByRole('button', { name: '隐藏 e2e-probe 曲线' })
  await expect(series).toHaveAttribute('aria-pressed', 'true')
  await analysis.getByRole('button', { name: '分析 e2e-probe-2' }).click()
  await expect(analysis.getByRole('button', { name: '分析 e2e-probe-2' })).toHaveAttribute('aria-pressed', 'true')
  await expect(series).toHaveAttribute('aria-pressed', 'true')
  await analysis.getByRole('combobox', { name: '分析任务' }).selectOption('e2e-probe')
  await expect(analysis.getByRole('button', { name: '分析 e2e-probe', exact: true })).toHaveAttribute('aria-pressed', 'true')
  const smoothing = page.getByRole('button', { name: '平滑峰值', exact: true })
  await smoothing.click()
  await expect(smoothing).toHaveAttribute('aria-pressed', 'true')
  for (const name of ['暂停刷新', '立即刷新', 'PNG', 'CSV'])
    await expect(page.getByRole('button', { name, exact: true }).locator('svg')).toHaveCount(1)
  const chart = page.locator('[aria-label="探测延迟和丢包曲线，TCP/HTTP 丢包表示探测失败率，左右轴分别为毫秒和百分比，长时间缺报为断点"]')
  await expect.poll(() => chart.evaluate(element => Math.round(element.getBoundingClientRect().height))).toBe(testInfo.project.name === 'mobile-chromium' ? 328 : 392)
  await chart.hover()
  await expect(chart.getByText('e2e-probe 延迟')).toBeVisible()
  await expect(chart.getByText('e2e-probe 丢包')).toBeVisible()
  await expect(chart.getByText('e2e-probe-2 丢包')).toBeVisible()
  await expect(chart.getByText('20 ms', { exact: true })).toBeVisible()
  await expect(chart.getByText('50.00%', { exact: true })).toBeVisible()
  const csvPromise = page.waitForEvent('download')
  await page.getByRole('button', { name: 'CSV', exact: true }).click()
  const csvDownload = await csvPromise
  expect(csvDownload.suggestedFilename()).toMatch(/^senulun-ping-[\w-]+-1h\.csv$/)
  const csv = await readFile(await csvDownload.path(), 'utf8')
  expect(csv).toContain('"e2e-probe 失败率(%)"')
  expect(csv).toContain('"e2e-probe-2 丢包率(%)"')
  expect(csv).toContain('"50"')
  const pngPromise = page.waitForEvent('download')
  await page.getByRole('button', { name: 'PNG', exact: true }).click()
  const pngDownload = await pngPromise
  expect(pngDownload.suggestedFilename()).toMatch(/^senulun-ping-[\w-]+-1h\.png$/)
  expect((await readFile(await pngDownload.path())).subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
  await page.locator('summary').filter({ hasText: '延迟与丢包率汇总' }).click()
  const summary = page.getByRole('table', { name: '所选时间范围的延迟与丢包率汇总' })
  await expect(summary.getByRole('columnheader', { name: '丢包率' })).toBeVisible()
  await expect(summary.getByRole('row', { name: /e2e-probe / })).toContainText('失败率 25.00%')
  await expect(summary.getByRole('row', { name: /e2e-probe-2 / })).toContainText('丢包率 50.00%')
  const layout = page.getByRole('group', { name: '图表布局' })
  await expect(layout.getByRole('button', { name: '合图' })).toHaveAttribute('aria-pressed', 'true')
  await layout.getByRole('button', { name: '分图' }).click()
  await expect(layout.getByRole('button', { name: '分图' })).toHaveAttribute('aria-pressed', 'true')
  await expect(chart).toHaveCount(0)
  const latencyChart = page.locator('[aria-label="独立延迟曲线，纵轴为毫秒，长时间缺报为断点"]')
  const failureChart = page.locator('[aria-label="独立丢包曲线，TCP/HTTP 丢包表示探测失败率，纵轴为百分比，长时间缺报为断点"]')
  await expect(latencyChart).toBeVisible()
  await expect(failureChart).toBeVisible()
  await failureChart.hover()
  await expect(failureChart.getByText('50.00%', { exact: true })).toBeVisible()
  const delay = page.getByRole('button', { name: '延迟', exact: true })
  await delay.click()
  await expect(latencyChart).toHaveCount(0)
  await expect(failureChart).toBeVisible()
  await delay.click()
  await expect(latencyChart).toBeVisible()
  await layout.getByRole('button', { name: '合图' }).click()
  await expect(chart).toBeVisible()
  await expect(failureChart).toHaveCount(0)
  await smoothing.click()
  await expect(smoothing).toHaveAttribute('aria-pressed', 'false')
  const failures = page.getByRole('button', { name: '丢包', exact: true })
  await failures.click()
  await expect(failures).toHaveAttribute('aria-pressed', 'false')
  await failures.click()
  await expect(failures).toHaveAttribute('aria-pressed', 'true')
  await series.click()
  await expect(analysis.getByRole('button', { name: '显示 e2e-probe 曲线' })).toHaveAttribute('aria-pressed', 'false')
  await analysis.getByRole('button', { name: '仅看此任务' }).click()
  await expect(analysis.getByRole('button', { name: '隐藏 e2e-probe 曲线' })).toHaveAttribute('aria-pressed', 'true')
  await expect(analysis.getByRole('button', { name: '显示 e2e-probe-2 曲线' })).toHaveAttribute('aria-pressed', 'false')
})

test('probe auto-refresh pauses without blocking range changes or manual refresh', async ({ page }) => {
  await page.clock.install()
  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.theme_settings = { ...payload.data.theme_settings, pingChartRefreshInterval: 10 }
    await route.fulfill({ response, json: payload })
  })
  const requests: string[] = []
  await page.route('**/api/v1/nodes/*/probes?*', async (route) => {
    requests.push(route.request().url())
    await route.fulfill({
      json: {
        tasks: [{ id: 'e2e-probe', name: 'e2e-probe', kind: 'tcp', interval_seconds: 60 }],
        records: [],
        chart: [],
        summary: [],
        chart_source_points: 0,
        chart_bucket_ms: 3601,
        range_start_unix_ms: Date.now() - 3_600_000,
        range_end_unix_ms: Date.now(),
      },
    })
  })
  await login(page)
  await page.getByRole('button', { name: '查看 e2e-node 节点详情' }).click()
  const pause = page.getByRole('button', { name: '暂停刷新' })
  await expect(pause).toBeVisible()
  await expect.poll(() => requests.length).toBe(1)
  await page.clock.runFor(9_000)
  expect(requests).toHaveLength(1)
  await page.clock.runFor(2_000)
  await expect.poll(() => requests.length).toBe(2)
  await pause.click()
  const resume = page.getByRole('button', { name: '自动刷新' })
  await expect(resume).toBeVisible()
  await expect(resume.locator('svg')).toHaveCount(1)
  await page.clock.runFor(11_000)
  expect(requests).toHaveLength(2)

  await page.getByRole('tablist', { name: '探测历史时间范围' }).getByRole('tab', { name: '6 小时' }).click()
  await expect.poll(() => requests.length).toBe(3)
  expect(requests[2]).toContain('hours=6')
  await page.getByRole('button', { name: '立即刷新' }).click()
  await expect.poll(() => requests.length).toBe(4)
  await page.clock.runFor(11_000)
  expect(requests).toHaveLength(4)

  await resume.click()
  expect(requests).toHaveLength(4)
  await expect(page.getByRole('button', { name: '立即刷新' })).toBeEnabled()
  await page.clock.runFor(9_000)
  expect(requests).toHaveLength(4)
  await page.clock.runFor(2_000)
  await expect.poll(() => requests.length).toBe(5)
  await page.evaluate(() => Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' }))
  await page.clock.runFor(11_000)
  expect(requests).toHaveLength(5)
  await page.evaluate(() => Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' }))
  await page.clock.runFor(11_000)
  await expect.poll(() => requests.length).toBe(6)
})
