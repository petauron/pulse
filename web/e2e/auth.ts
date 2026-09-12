import type { APIRequestContext, Page } from '@playwright/test'
import { expect } from '@playwright/test'
import { testPassword, testUsername } from './credentials.mjs'

export async function login(page: Page, destination = '/'): Promise<void> {
  await expect.poll(async () => {
    const response = await page.request.get('/api/auth/status')
    return response.ok() && (await response.json()).initialized
  }, { timeout: 20_000 }).toBe(true)
  await page.goto(`/login?redirect=${encodeURIComponent(destination)}`)
  await page.getByLabel('用户名', { exact: true }).fill(testUsername)
  await page.getByLabel('密码', { exact: true }).fill(testPassword)
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page).toHaveURL(destination)
  await expect(page).toHaveTitle('Pulse')
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
}

export async function mutation(request: APIRequestContext, path: string, data: unknown = {}) {
  const statusResponse = await request.get('/api/auth/status')
  expect(statusResponse.ok()).toBe(true)
  const status = await statusResponse.json()
  return request.post(path, {
    headers: { 'Origin': 'http://127.0.0.1:18080', 'X-CSRF-Token': status.csrf_token },
    data,
  })
}

export async function waitForNode(request: APIRequestContext): Promise<string> {
  await expect.poll(async () => {
    const response = await request.get('/api/admin/state')
    const state = await response.json()
    return state.nodes.find((node: { name: string }) => node.name === 'e2e-node')?.id ?? ''
  }, { timeout: 20_000 }).not.toBe('')
  const state = await (await request.get('/api/admin/state')).json()
  return state.nodes.find((node: { name: string }) => node.name === 'e2e-node').id
}
