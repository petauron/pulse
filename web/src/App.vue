<script setup lang="ts">
import { onMounted, onUnmounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Toaster } from '@/components/ui/sonner'
import { useAppStore } from '@/stores/app'
import { useAuthStore } from '@/stores/auth'
import { destroyInitManager, initApp } from '@/utils/init'
import { AUTH_REQUIRED_EVENT } from '@/utils/session'
import Background from './components/Background.vue'
import Footer from './components/Footer.vue'
import Header from './components/Header.vue'
import LoadingCover from './components/LoadingCover.vue'
import Provider from './components/Provider.vue'

const appStore = useAppStore()
const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

async function syncDashboard(): Promise<void> {
  if (route.name === 'login') {
    destroyInitManager()
    appStore.loading = false
    return
  }
  await initApp()
}

function onAuthRequired(): void {
  destroyInitManager()
  auth.clear()
  appStore.loading = false
  if (route.name !== 'login')
    void router.replace({ name: 'login', query: { redirect: route.fullPath } })
}

onMounted(async () => {
  window.addEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
  await router.isReady()
  await syncDashboard()
})
watch(() => route.name, () => void syncDashboard())

onUnmounted(() => {
  window.removeEventListener(AUTH_REQUIRED_EVENT, onAuthRequired)
  destroyInitManager()
})
</script>

<template>
  <Provider>
    <a
      href="#main-content"
      class="sr-only fixed left-4 top-4 z-50 rounded bg-background px-3 py-2 text-sm focus:not-sr-only"
    >跳到主要内容</a>
    <Background />
    <LoadingCover v-if="appStore.loading" />
    <Header />
    <main v-if="!appStore.loading" id="main-content" tabindex="-1" class="flex-1">
      <div class="max-w-[1280px] mx-auto">
        <RouterView v-slot="{ Component }">
          <KeepAlive :key="auth.epoch" :include="['HomeView']">
            <component :is="Component" />
          </KeepAlive>
        </RouterView>
      </div>
    </main>
    <Footer v-if="!appStore.loading" />
    <Toaster rich-colors close-button position="top-center" />
  </Provider>
</template>
