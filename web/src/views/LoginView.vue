<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Input } from '@/components/ui/input'
import { useAuthStore } from '@/stores/auth'
import { getSharedApi } from '@/utils/api'
import { loginDestination } from '@/utils/session'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const token = ref('')
const username = ref('')
const password = ref('')
const code = ref('')
const busy = ref(false)
const error = ref('')
const setup = computed(() => auth.status?.initialized === false)
const oauthTotp = computed(() => route.query.oauth_totp === 'required' || auth.status?.oauth_totp_required === true)
const heading = computed(() => setup.value ? '初始化 Pulse' : oauthTotp.value ? '完成两步验证' : '登录 Pulse')

async function refresh(): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    await auth.refresh()
  }
  catch (cause) { error.value = cause instanceof Error ? cause.message : String(cause) }
  finally { busy.value = false }
}

onMounted(refresh)

async function submit(): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    const path = setup.value ? 'setup' : oauthTotp.value ? 'oauth/complete' : 'login'
    const body = setup.value
      ? { token: token.value, username: username.value, password: password.value }
      : oauthTotp.value ? { code: code.value } : { username: username.value, password: password.value, code: code.value || undefined }
    await auth.authenticate(path, body)
    token.value = ''
    password.value = ''
    code.value = ''
    await router.replace(loginDestination(route.query.redirect))
  }
  catch (cause) { error.value = cause instanceof Error ? cause.message : String(cause) }
  finally { busy.value = false }
}

async function startOAuth(): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    const { authorization_url } = await getSharedApi().post<{ authorization_url: string }>('auth/oauth/start')
    const url = new URL(authorization_url)
    if (url.protocol !== 'https:' && !(url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)))
      throw new Error('OAuth 服务返回了无效的登录地址')
    window.location.assign(url.href)
  }
  catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
    busy.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-md px-4 py-8">
    <CardX :title="heading" content-class="space-y-4">
      <p v-if="setup" class="text-sm text-muted-foreground">
        使用部署时配置的初始化令牌创建管理员。密码至少 12 个字符。
      </p>
      <p v-else-if="oauthTotp" class="text-sm text-muted-foreground">
        输入身份验证器中的六位验证码以完成 OAuth 登录。
      </p>
      <p v-if="error" id="login-error" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
        {{ error }}
      </p>
      <form v-if="auth.status" class="space-y-4" :aria-busy="busy" :aria-describedby="error ? 'login-error' : undefined" @submit.prevent="submit">
        <div v-if="setup" class="space-y-2">
          <label for="setup-token" class="text-sm font-medium">初始化令牌</label>
          <Input id="setup-token" v-model="token" type="password" autocomplete="off" required :disabled="busy" />
        </div>
        <template v-if="!oauthTotp">
          <div class="space-y-2">
            <label for="username" class="text-sm font-medium">用户名</label>
            <Input id="username" v-model="username" name="username" autocomplete="username" required maxlength="64" pattern="[A-Za-z0-9_.\-]+" :disabled="busy" />
            <p v-if="setup" class="text-xs text-muted-foreground">
              使用英文字母、数字、下划线、连字符或句点。
            </p>
          </div>
          <div class="space-y-2">
            <label for="password" class="text-sm font-medium">密码</label>
            <Input id="password" v-model="password" name="password" type="password" :autocomplete="setup ? 'new-password' : 'current-password'" :minlength="setup ? 12 : undefined" maxlength="1024" required :disabled="busy" />
          </div>
        </template>
        <div v-if="!setup" class="space-y-2">
          <label for="login-code" class="text-sm font-medium">两步验证码{{ oauthTotp ? '' : '（已开启时填写）' }}</label>
          <Input id="login-code" v-model="code" name="code" autocomplete="one-time-code" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" :required="oauthTotp" :disabled="busy" />
        </div>
        <Button class="min-h-11 w-full" type="submit" :disabled="busy">
          {{ busy ? '正在处理…' : setup ? '创建管理员' : '登录' }}
        </Button>
      </form>
      <Button v-else class="min-h-11 w-full" :disabled="busy" @click="refresh">
        {{ busy ? '正在连接…' : '重新连接' }}
      </Button>
      <Button v-if="auth.status?.oauth_enabled && !oauthTotp" variant="outline" class="min-h-11 w-full" :disabled="busy" @click="startOAuth">
        使用 OAuth 登录
      </Button>
      <RouterLink v-if="auth.status?.initialized && !oauthTotp" to="/" class="block text-center text-sm underline underline-offset-4">
        返回监控首页
      </RouterLink>
    </CardX>
  </div>
</template>
