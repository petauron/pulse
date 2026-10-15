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
  const card = page.getByRole('button', { name: '查看 e2e-node 节点详情' })
  await expect(card).toBeVisible({ timeout: 20_000 })
  await expect(card).toContainText('e2e-node')
  await expect(card).toContainText('未配置探测')
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
  await expect(page.getByRole('region', { name: '剩余价值详情' })).toBeVisible()
  await expect(page.getByRole('button', { name: '关闭价值详情' })).toBeFocused()
  await expect(page.getByText('月均支出', { exact: true })).toBeVisible()
  await page.getByRole('combobox', { name: '切换汇率基准币种' }).selectOption('USD')
  await expect(valueTrigger).toContainText('USD')
  await page.keyboard.press('Escape')
  await expect(valueTrigger).toHaveAttribute('aria-expanded', 'false')
  await expect(valueTrigger).toBeFocused()

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
  await expect(page.getByText('e2e-node', { exact: true })).toBeVisible()
  await expect(page.getByText('在线', { exact: true })).toBeVisible()
  await expect(page.getByText('CPU', { exact: true }).first()).toBeVisible()
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

test('network quality offers the full retained range with an exact custom label', async ({ page }) => {
  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.ping_record_preserve_time = 2400
    await route.fulfill({ response, json: payload })
  })
  await login(page, '/network-quality')
  const ranges = page.getByRole('group', { name: '统计范围' })
  await expect(ranges.getByRole('button')).toHaveText(['1 小时', '6 小时', '12 小时', '1 天', '7 天', '30 天', '60 天', '90 天', '100 天'])
  await ranges.getByRole('button', { name: '100 天' }).click()
  await expect(ranges.getByRole('button', { name: '100 天' })).toHaveAttribute('aria-pressed', 'true')
})

test('probe task information opens without changing the selected chart series', async ({ page }) => {
  await page.route('**/api/v1/nodes/*/probes?*', async (route) => {
    await route.fulfill({
      json: {
        tasks: [{ id: 'e2e-probe', name: 'e2e-probe', kind: 'tcp', interval_seconds: 60 }],
        records: [{ task_id: 'e2e-probe', collected_at_unix_ms: Date.now(), received_at_unix_ms: Date.now(), latency_ms: 20, success: true, error: null }],
        chart: [{ task_id: 'e2e-probe', received_at_unix_ms: Date.now(), latency_ms: 20, failed_samples: 0, samples: 1 }],
        summary: [{ task_id: 'e2e-probe', samples: 4, loss_percent: 25, avg_latency_ms: 20, min_latency_ms: 10, max_latency_ms: 30, latest_latency_ms: 20, p50_latency_ms: 20, p95_latency_ms: 27, p99_latency_ms: 29, p99_p50_ratio: 0.45 }],
        chart_source_points: 1,
        chart_bucket_ms: 3601,
        range_start_unix_ms: Date.now() - 3_600_000,
        range_end_unix_ms: Date.now(),
      },
    })
  })
  await login(page)
  await page.getByRole('button', { name: '查看 e2e-node 节点详情' }).click()
  const series = page.getByRole('button', { name: /^e2e-probe，平均延迟/ })
  await expect(series).toHaveAttribute('aria-pressed', 'true')
  await expect(series).toContainText('P95')
  await expect(series).toContainText('27ms')
  const information = page.getByRole('button', { name: /^e2e-probe，最低/ })
  await information.focus()
  const tooltip = information.locator('..').getByRole('tooltip')
  await expect(tooltip).toContainText('P99')
  await expect(tooltip).toContainText('P95')
  await expect(tooltip).toContainText('0.45')
  await information.click()
  await expect(series).toHaveAttribute('aria-pressed', 'true')
  const smoothing = page.getByRole('button', { name: '平滑峰值', exact: true })
  await smoothing.click()
  await expect(smoothing).toHaveAttribute('aria-pressed', 'true')
  const chart = page.locator('[aria-label="探测延迟曲线，长时间缺报为断点，可显示失败标记"]')
  await chart.hover()
  await expect(chart.getByText('20 ms', { exact: true })).toBeVisible()
  await smoothing.click()
  await expect(smoothing).toHaveAttribute('aria-pressed', 'false')
  await series.click()
  await expect(series).toHaveAttribute('aria-pressed', 'false')
})
