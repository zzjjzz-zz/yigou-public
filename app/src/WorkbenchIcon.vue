<script setup lang="ts">
import { icons } from './icons'
defineProps<{name:string}>()
</script>
<template><svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" v-html="icons[name]||icons.server"/></template>
<style scoped>.icon{width:18px;height:18px;display:inline-block;flex-shrink:0;vertical-align:middle}</style>
