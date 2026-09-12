import { ref } from 'vue'

export function useAdminAction(refresh?: () => Promise<void>) {
  const busy = ref(false)
  const error = ref('')
  const success = ref('')

  async function run(action: () => Promise<void>, message = '已保存', reload = true): Promise<void> {
    if (busy.value)
      return
    busy.value = true
    error.value = ''
    success.value = ''
    try {
      await action()
      success.value = message
      if (refresh && reload) {
        try {
          await refresh()
        }
        catch { error.value = '操作已完成，但刷新数据失败，请手动刷新。' }
      }
    }
    catch (cause) { error.value = cause instanceof Error ? cause.message : String(cause) }
    finally { busy.value = false }
  }

  return { busy, error, success, run }
}
