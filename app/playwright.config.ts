import { defineConfig } from '@playwright/test'
import { existsSync, realpathSync } from 'node:fs'
import { isAbsolute, relative, resolve } from 'node:path'

// An isolated browser-test copy must install its own dependencies from the
// lockfile. A node_modules junction/symlink from another checkout resolves
// outside this project, and Vite then answers every font request with 403 so
// the suite can pass while silently rendering fallback fonts. Fail fast instead.
const projectRoot = realpathSync(resolve('.'))
const modulesPath = resolve('node_modules')
if (existsSync(modulesPath)) {
  const modulesRoot = realpathSync(modulesPath)
  const relativeModules = relative(projectRoot, modulesRoot)
  if (relativeModules.startsWith('..') || isAbsolute(relativeModules))
    throw new Error('app/node_modules resolves outside this checkout (' + modulesRoot + '). Run "pnpm --dir app install --frozen-lockfile" in this copy; do not junction/symlink another checkout\'s node_modules.')
}

// Reuse an installed browser when Playwright's managed revision is not cached.
// An explicit environment override always wins; otherwise Playwright uses its
// pinned browser on systems without a known system Chrome/Chromium binary.
const browser = process.env.YIGOU_CHROMIUM || ['/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser'].find(existsSync)

export default defineConfig({testDir:'../tests/ui',outputDir:'../builds/ui-results',workers:1,reporter:[['list'],['json',{outputFile:'../builds/ui-results.json'}]],use:{baseURL:'http://127.0.0.1:1420',viewport:{width:1280,height:850},launchOptions:{executablePath:browser}},webServer:{command:'pnpm dev',url:'http://127.0.0.1:1420',reuseExistingServer:!process.env.CI,timeout:30000}})
