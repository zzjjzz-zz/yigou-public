<script setup lang="ts">
import { computed } from 'vue'
import WorkbenchIcon from './WorkbenchIcon.vue'
import { namedRole, type DeviceRole } from './device-presentation'
const props=defineProps<{name:string;role?:DeviceRole}>()
// Name-derived styling is decorative; ports do not represent measured port states.
const kind=computed(()=>props.role||namedRole(props.name))
const core=computed(()=>kind.value==='core')
</script>
<template><span v-if="['core','access','switch'].includes(kind)" class="device-glyph switch-chassis" :class="core?'is-core':'is-access'" aria-hidden="true"><span class="chassis-rail"><i/><i/></span><span class="chassis-ports"><i v-for="n in core?8:6" :key="n"/></span></span><span v-else class="device-glyph other-chassis" aria-hidden="true"><WorkbenchIcon :name="kind==='router'?'router':kind==='firewall'?'shield':'server'"/></span></template>
<style scoped>
.switch-chassis{width:45px;height:29px;min-width:45px;display:flex;align-items:center;justify-content:center;gap:3px;padding:4px 3px;border:1px solid var(--border);border-radius:5px;background:var(--surface-raised);box-shadow:0 1px 0 color-mix(in srgb,var(--fg) 11%,transparent)}
.is-core{border-left:3px solid var(--accent);background:var(--surface)}.is-access{border-top:2px solid var(--muted)}
.chassis-rail{display:flex;flex-direction:column;gap:3px}.chassis-rail i{width:3px;height:3px;border:1px solid var(--muted);border-radius:50%}
.chassis-ports{display:grid;grid-template-columns:repeat(4,5px);grid-auto-rows:5px;gap:2px}.chassis-ports i{width:5px;height:5px;border:1px solid var(--muted);border-radius:1px;background:var(--surface)}.is-access .chassis-ports{grid-template-columns:repeat(3,5px)}
.other-chassis{width:45px;height:29px;min-width:45px;display:grid;place-items:center;border:1px solid var(--line);border-radius:5px;background:var(--surface-raised);color:var(--muted)}
.device-glyph.topology-glyph{width:23px;min-width:23px;height:17px;padding:2px;gap:2px;border-radius:3px}.topology-glyph .chassis-rail{display:none}.topology-glyph .chassis-ports{grid-template-columns:repeat(4,3px);grid-auto-rows:3px;gap:1px}.topology-glyph .chassis-ports i{width:3px;height:3px}.topology-glyph.is-access .chassis-ports{grid-template-columns:repeat(3,3px)}.topology-glyph :deep(.icon){width:16px;height:16px}
.device-glyph.mobile-glyph{width:31px;min-width:31px;height:25px;padding:3px;gap:2px}.mobile-glyph .chassis-rail{gap:2px}.mobile-glyph .chassis-rail i{width:2px;height:2px}.mobile-glyph .chassis-ports{grid-template-columns:repeat(4,3px);grid-auto-rows:3px;gap:2px}.mobile-glyph .chassis-ports i{width:3px;height:3px}.mobile-glyph.is-access .chassis-ports{grid-template-columns:repeat(3,3px)}.mobile-glyph :deep(.icon){width:16px;height:16px}
</style>
