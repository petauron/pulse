<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useAdminAction } from '@/composables/useAdminAction'
import { useAuthStore } from '@/stores/auth'
import { getSharedApi } from '@/utils/api'
import { destroyInitManager } from '@/utils/init'

const auth = useAuthStore()
const router = useRouter()
const currentPassword = ref('')
const newPassword = ref('')
const code = ref('')
const totpPassword = ref('')
const totpCode = ref('')
const pendingTotp = ref<{ secret: string, otpauth_url: string, expires_in: number } | null>(null)
const confirm = ref<'password' | 'disable' | null>(null)
const { busy, error, run } = useAdminAction()

async function signInAgain(): Promise<void> {
  currentPassword.value = ''
  newPassword.value = ''
  totpPassword.value = ''
  pendingTotp.value = null
  destroyInitManager()
  auth.clear()
  await router.replace({ name: 'login', query: { redirect: '/admin?section=security' } })
}

async function setupTotp(): Promise<void> {
  await run(async () => {
    pendingTotp.value = await getSharedApi().post('auth/totp/setup', { password: totpPassword.value })
    totpPassword.value = ''
  }, '')
}

async function enableTotp(): Promise<void> {
  await run(async () => {
    await getSharedApi().post('auth/totp/enable', { code: totpCode.value })
    await signInAgain()
  }, '')
}

async function execute(): Promise<void> {
  await run(async () => {
    if (confirm.value === 'password')
      await getSharedApi().post('auth/password', { current_password: currentPassword.value, new_password: newPassword.value, code: code.value || undefined })
    else
      await getSharedApi().post('auth/totp/disable', { password: totpPassword.value, code: totpCode.value })
    confirm.value = null
    await signInAgain()
  }, '')
}
</script>

<template>
  <div class="space-y-4">
    <p v-if="error" role="alert" class="text-sm text-destructive">
      {{ error }}
    </p>
    <CardX title="修改密码" content-class="space-y-4">
      <p class="text-sm text-muted-foreground">
        修改密码后所有会话都会注销，需要重新登录。
      </p>
      <form class="max-w-lg space-y-4" @submit.prevent="confirm = 'password'">
        <fieldset :disabled="busy" class="space-y-4">
          <input :value="auth.status?.username" type="text" name="username" autocomplete="username" class="sr-only" tabindex="-1" aria-label="用户名" readonly>
          <label class="grid gap-2 text-sm">当前密码<Input v-model="currentPassword" type="password" autocomplete="current-password" required maxlength="1024" /></label>
          <label class="grid gap-2 text-sm">新密码（至少 12 个字符）<Input v-model="newPassword" type="password" autocomplete="new-password" required minlength="12" maxlength="1024" /></label>
          <label v-if="auth.status?.totp_enabled" class="grid gap-2 text-sm">两步验证码<Input v-model="code" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" required /></label>
        </fieldset>
        <Button type="submit" class="min-h-11" :disabled="busy">
          修改密码
        </Button>
      </form>
    </CardX>
    <CardX title="两步验证（TOTP）" content-class="space-y-4">
      <p class="text-sm">
        当前状态：{{ auth.status?.totp_enabled ? '已启用' : '未启用' }}
      </p>
      <p class="text-sm text-muted-foreground">
        使用支持 TOTP 的身份验证器。验证码只可使用一次；连续操作请等待新的验证码。
      </p>
      <form v-if="auth.status?.totp_enabled" class="max-w-lg space-y-4" @submit.prevent="confirm = 'disable'">
        <label class="grid gap-2 text-sm">当前密码<Input v-model="totpPassword" type="password" autocomplete="current-password" required :disabled="busy" /></label>
        <label class="grid gap-2 text-sm">两步验证码<Input v-model="totpCode" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" required :disabled="busy" /></label>
        <Button type="submit" variant="outline" class="min-h-11" :disabled="busy">
          关闭两步验证
        </Button>
      </form>
      <form v-else-if="!pendingTotp" class="max-w-lg space-y-4" @submit.prevent="setupTotp">
        <label class="grid gap-2 text-sm">确认当前密码<Input v-model="totpPassword" type="password" autocomplete="current-password" required :disabled="busy" /></label>
        <Button type="submit" class="min-h-11" :disabled="busy">
          开始设置
        </Button>
      </form>
      <form v-else class="max-w-lg space-y-4" @submit.prevent="enableTotp">
        <p class="text-sm text-muted-foreground">
          将以下密钥添加到身份验证器，并在 {{ Math.floor(pendingTotp.expires_in / 60) }} 分钟内输入验证码确认。密钥不会发送给任何二维码服务。
        </p>
        <label class="grid gap-2 text-sm">身份验证器密钥<Input :model-value="pendingTotp.secret" readonly autocomplete="off" class="font-mono" @focus="($event.target as HTMLInputElement).select()" /></label>
        <label class="grid gap-2 text-sm">六位验证码<Input v-model="totpCode" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" required :disabled="busy" /></label>
        <p class="text-sm text-muted-foreground">
          启用成功后将注销全部会话，请使用密码及验证码重新登录。
        </p>
        <div class="flex gap-2">
          <Button type="submit" class="min-h-11" :disabled="busy">
            确认启用
          </Button><Button type="button" variant="outline" :disabled="busy" @click="pendingTotp = null; totpCode = ''">
            取消设置
          </Button>
        </div>
      </form>
    </CardX>
    <Dialog :open="confirm !== null" @update:open="value => { if (!value && !busy) confirm = null }">
      <DialogContent :show-close="!busy" @escape-key-down="event => { if (busy) event.preventDefault() }" @interact-outside="event => { if (busy) event.preventDefault() }">
        <DialogHeader><DialogTitle>{{ confirm === 'password' ? '确认修改密码' : '确认关闭两步验证' }}</DialogTitle><DialogDescription>此操作会注销全部会话。{{ confirm === 'disable' ? '关闭后登录将不再需要身份验证器。' : '请确保已保存新密码。' }}</DialogDescription></DialogHeader><p v-if="error" role="alert" class="text-sm text-destructive">
          {{ error }}
        </p><DialogFooter>
          <Button variant="outline" :disabled="busy" @click="confirm = null">
            取消
          </Button><Button variant="destructive" :disabled="busy" @click="execute">
            {{ busy ? '正在处理…' : '确认' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
