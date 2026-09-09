import { createRequire } from 'node:module'
import { resolve } from 'node:path'
import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'
import { viteStaticCopy } from 'vite-plugin-static-copy'

const require = createRequire(import.meta.url)
const process = require('node:process')
const packageJson = require('./package.json')

function shouldIgnoreRollupWarning(warning: { code?: string, id?: string }): boolean {
  return warning.code === 'INVALID_ANNOTATION'
    && warning.id?.includes('/node_modules/@vueuse/core/dist/index.js') === true
}

export default defineConfig({
  define: {
    __BUILD_VERSION__: JSON.stringify(packageJson.version),
    __BUILD_GIT_HASH__: JSON.stringify(process.env.PULSE_BUILD_GIT_HASH || 'dev'),
  },
  plugins: [
    vue(),
    tailwindcss(),
    viteStaticCopy({
      targets: [
        {
          src: resolve(__dirname, 'node_modules/flag-icons/flags/4x3/*.svg'),
          dest: 'assets/flags',
        },
      ],
    }),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    host: '127.0.0.1',
    proxy: {
      '/api': 'http://127.0.0.1:8080',
    },
  },
  build: {
    license: {
      fileName: 'THIRD_PARTY_LICENSES.md',
    },
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      onwarn(warning, defaultHandler) {
        if (shouldIgnoreRollupWarning(warning))
          return
        defaultHandler(warning)
      },
      output: {
        manualChunks(id) {
          if (!id.includes('/node_modules/'))
            return undefined
          if (id.includes('/echarts/') || id.includes('/zrender/') || id.includes('/vue-echarts/'))
            return 'echarts'
          if (id.includes('/vue/') || id.includes('/vue-router/') || id.includes('/pinia/'))
            return 'vue-vendor'
          if (id.includes('/reka-ui/'))
            return 'reka-ui'
          if (id.includes('/@vueuse/'))
            return 'vueuse'
          return undefined
        },
      },
    },
  },
})
