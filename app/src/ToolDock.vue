<script setup lang="ts">
import WorkbenchIcon from './WorkbenchIcon.vue'
defineProps<{panel:string;height:number;busy:string;expanded:boolean}>()
const emit=defineEmits<{select:[panel:string];resize:[event:PointerEvent];step:[delta:number]}>()
const tabs=[['terminal','terminal-2','SSH 终端'],['tasks','history','执行记录'],['history','database','加密备份'],['diff','git-compare','配置差异'],['details','file-code','设备详情']]
</script>
<template>
  <section class="results tool-dock" :class="{expanded}" aria-label="设备工具" :style="{'--dock-height-override':height?height+'px':undefined}">
    <div v-if="!expanded" class="dock-resize" role="separator" aria-orientation="horizontal" aria-label="调整工具区高度" :aria-valuenow="height||192" :aria-valuemin="146" :aria-valuemax="520" tabindex="0" @pointerdown="emit('resize',$event)" @keydown.up.prevent="emit('step',24)" @keydown.down.prevent="emit('step',-24)"><span/></div>
    <div class="result-toolbar"><span class="dock-title">设备工具</span><nav class="tabs" aria-label="工具标签"><button v-for="[id,icon,label] in tabs" :key="id" :class="{active:panel===id}" :aria-current="panel===id?'page':undefined" @click="emit('select',id)"><WorkbenchIcon :name="icon"/>{{label}}</button></nav><span v-if="busy" class="busy" role="status">{{busy}}…</span><div class="dock-actions"><slot name="actions"/></div></div>
    <div class="dock-content"><slot/></div>
  </section>
</template>
<style scoped>
.tool-dock{background:var(--surface)}.result-toolbar{display:flex;flex-shrink:0;overflow-x:auto;gap:5px}.dock-title{min-width:66px;padding-right:14px;margin-right:8px;border-right:1px solid var(--line);font-size:12px;font-weight:650;white-space:nowrap}.tabs{gap:0;flex-shrink:0}.tabs button{display:inline-flex;align-items:center;gap:7px;padding:0 12px;white-space:nowrap;min-height:44px;font-size:12px}.tabs .icon{width:15px;height:15px}.tabs button.active{background:var(--accent-soft);font-weight:650}.dock-actions{margin-left:auto;flex-shrink:0}.dock-content{display:flex;flex:1;min-height:0;overflow:auto;flex-direction:column}.dock-content :deep(.terminal-container){flex:1;min-height:0}.dock-content :deep(.terminal-container .output-empty){margin:auto}.busy{white-space:nowrap;font-size:12px}
@media(max-width:760px){.dock-title,.busy{display:none}.tabs{flex:1}.tabs button{padding:0 9px;flex:1;gap:4px}.tabs .icon{display:none}.result-toolbar{padding:0 9px!important;gap:0}.dock-actions{margin-left:auto}.dock-actions :deep(button){padding:6px;min-width:31px;min-height:36px}.dock-actions :deep(button span){display:none}}
</style>
