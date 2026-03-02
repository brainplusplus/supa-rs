import { defineConfig } from 'vitest/config'
import { loadEnv } from 'vite'

export default defineConfig(({ mode }) => {
  // Load .env + .env.{mode} + .env.{mode}.local from test-client/
  const env = loadEnv(mode, process.cwd(), '')

  // Bridge: globalSetup.js runs in Node context (outside Vite transform).
  // It cannot read import.meta.env — so we pass the mode via process.env.
  process.env.__TEST_MODE = mode

  return {
    test: {
      globals: true,
      testTimeout: 15000,
      hookTimeout: 10000,
      globalSetup: './globalSetup.js',
      env,  // injected as import.meta.env.* in all test files
    },
  }
})
