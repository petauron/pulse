import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test('real Agent is visible through the Emerald card, list, search, detail, and history flow', async ({ page }, testInfo) => {
  const consoleErrors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error')
      consoleErrors.push(message.text())
  })

  await page.goto('/')
  const card = page.getByRole('button', { name: '查看 e2e-node 节点详情' })
  await expect(card).toBeVisible({ timeout: 20_000 })
  await expect(card).toContainText('e2e-node')
  await expect(card).toContainText('N/A')
  const flag = card.getByRole('img', { name: 'SG', exact: true })
  await expect(flag).toHaveAttribute('src', '/assets/flags/sg.svg')
  await expect.poll(() => flag.evaluate(image => (image as HTMLImageElement).naturalWidth)).toBeGreaterThan(0)
  const cardScreenshot = testInfo.outputPath('node-card.png')
  await page.screenshot({ path: cardScreenshot, fullPage: true })
  await testInfo.attach('node-card', { path: cardScreenshot, contentType: 'image/png' })

  const statusTrigger = page.getByRole('button', { name: '查看节点状态汇总' })
  await statusTrigger.focus()
  await statusTrigger.press('Enter')
  await expect(statusTrigger).toHaveAttribute('aria-expanded', 'true')
  await expect(page.getByRole('region', { name: '节点状态汇总' })).toBeVisible()
  await expect(page.getByRole('button', { name: '关闭节点状态' })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(statusTrigger).toHaveAttribute('aria-expanded', 'false')
  await expect(statusTrigger).toBeFocused()

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
