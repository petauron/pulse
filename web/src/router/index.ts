import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { loginDestination } from '@/utils/session'

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
    },
    {
      path: '/admin',
      name: 'admin',
      component: () => import('@/views/AdminView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/',
      name: 'home',
      component: () => import('@/views/HomeView.vue'),
    },
    {
      path: '/instance/:id',
      name: 'instance-detail',
      component: () => import('@/views/InstanceDetail.vue'),
    },
  ],
})

router.beforeEach(async (to) => {
  if (to.query.oauth_totp === 'required' && to.name !== 'login')
    return { name: 'login', query: { oauth_totp: 'required' } }
  const auth = useAuthStore()
  try {
    const status = auth.status ?? await auth.refresh()
    if ((!status.initialized || (to.meta.requiresAuth && !status.logged_in)) && to.name !== 'login')
      return { name: 'login', query: { redirect: to.fullPath } }
    if (to.name === 'login' && status.logged_in)
      return loginDestination(to.query.redirect)
  }
  catch {
    if (to.meta.requiresAuth)
      return { name: 'login', query: { redirect: to.fullPath } }
  }
})

export default router
