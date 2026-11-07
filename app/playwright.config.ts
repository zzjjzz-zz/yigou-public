import { defineConfig } from '@playwright/test'
import { existsSync } from 'node:fs'

// Reuse an installed browser when Playwright's managed revision is not cached.
// An explicit environment override always wins; otherwise Playwright uses its
// pinned browser on systems without a known system Chrome/Chromium binary.
const browser = process.env.YIGOU_CHROMIUM || ['/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser'].find(existsSync)

export default defineConfig({testDir:'../tests/ui',outputDir:'../builds/ui-results',workers:1,reporter:[['list'],['json',{outputFile:'../builds/ui-results.json'}]],use:{baseURL:'http://127.0.0.1:1420',viewport:{width:1280,height:850},launchOptions:{executablePath:browser}},webServer:{command:'pnpm dev',url:'http://127.0.0.1:1420',reuseExistingServer:!process.env.CI,timeout:30000}})
