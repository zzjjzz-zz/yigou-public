import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
export default defineConfig({ plugins: [vue()], clearScreen: false, envPrefix: ['VITE_'], build: { target: 'es2022', assetsInlineLimit: 0 } })
