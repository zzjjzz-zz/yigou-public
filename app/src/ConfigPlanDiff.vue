<script setup lang="ts">
import { computed } from 'vue'
import type { ConfigPlan } from './api'
import WorkbenchIcon from './WorkbenchIcon.vue'
const props=defineProps<{plan:ConfigPlan;deviceName:string;busy:boolean}>()
const emit=defineEmits<{review:[]}>()
// Explain only literal command forms we recognize, never infer a successful device change.
const changes=computed(()=>{
  const entries:string[]=[]
  let other=0
  for(const raw of props.plan.config.commands){
    const command=raw.trim();let match:RegExpMatchArray|null
    if((match=command.match(/^vlan\s+(\d+)$/i)))entries.push(`配置编号为 ${match[1]} 的 VLAN 网络`)
    else if((match=command.match(/^vlan batch\s+([\d\s-]+)$/i)))entries.push(`配置 VLAN 编号范围 ${match[1]}`)
    else if((match=command.match(/^(?:name|description)\s+(.+)$/i)))entries.push(`将当前配置项的名称或说明设为“${match[1]}”`)
    else if((match=command.match(/^interface\s+(.+)$/i)))entries.push(`进入接口 ${match[1]} 的配置范围`)
    else if((match=command.match(/^(?:port|switchport)\s+(?:access\s+vlan|default\s+vlan)\s+(\d+)$/i)))entries.push(`将当前接口的接入 VLAN 设为 ${match[1]}`)
    else if((match=command.match(/^port\s+link-type\s+(access|trunk|hybrid)$/i)))entries.push(`将当前接口的链路类型设为 ${match[1]}`)
    else if(/^(?:quit|exit|return|system-view|configure terminal|end)$/i.test(command))continue
    else other++
  }
  if(other)entries.push(`另外包含 ${other} 条需逐条核对的命令，请展开原始内容`)
  return [...new Set(entries)]
})
const status=computed(()=>props.plan.status==='pending'?'待审批 · 未执行':props.plan.status==='consumed'?'已提交 · 结果见执行记录':props.plan.status==='invalidated'?'设备会话已变化 · 计划失效':props.plan.status)
</script>
<template>
  <div class="diff-explained">
    <div class="diff-intro"><span class="diff-label"><WorkbenchIcon name="git-compare"/>配置变化说明</span><span class="plan-status">{{status}}</span></div>
    <h3>准备变更 {{deviceName}} 的网络配置</h3>
    <p>以下说明来自后端生成并绑定设备的配置计划。共 {{plan.config.commands.length}} 条命令；{{plan.config.save?'计划保存启动配置':'不保存启动配置'}}。</p>
    <div class="diff-simple"><div><span>变更前</span><strong>本面板未读取执行前配置</strong><small>不能由计划推断当前设备已经具备或缺少哪些配置。</small></div><div><span>计划变更</span><ul><li v-for="(change,index) in changes" :key="index">{{change}}</li></ul><strong v-if="!changes.length">请展开核对具体命令</strong></div></div>
    <p class="diff-caution"><WorkbenchIcon name="shield"/>中文说明便于审阅，不能代替原始命令、备份及回读。只有完成最终审批，后端才会尝试执行同一份计划。</p>
    <details class="diff-raw"><summary>查看原始配置差异和命令（可选）<WorkbenchIcon name="chevron-down"/></summary><pre>--- 当前配置尚未读取
+++ 待审批命令
<span v-for="(command,index) in plan.config.commands" :key="index" class="diff-add">+ {{command}}
</span></pre><div class="readback-checks"><strong>执行后回读条件</strong><p v-for="(check,index) in plan.config.checks" :key="index">{{check.query}}：包含 {{check.contains.join('、')||'无'}}；不包含 {{check.not_contains.join('、')||'无'}}</p></div><p class="plan-hash">计划校验值：{{plan.request_hash}}</p></details>
    <button class="control" :disabled="busy||plan.status!=='pending'" @click="emit('review')">审阅风险并审批 <WorkbenchIcon name="arrow-right"/></button>
  </div>
</template>
<style scoped>
.diff-explained{max-width:900px;display:grid;gap:12px;color:var(--fg);font-size:13px}.diff-intro{display:flex;align-items:center;justify-content:space-between;gap:10px}.diff-label{display:inline-flex;align-items:center;gap:7px;font-size:12px;color:var(--muted)}h3{font-size:16px;font-weight:650;line-height:1.4}p{color:var(--muted);font-size:13px;line-height:1.8}.plan-status{font-size:12px;border:1px solid var(--line);border-radius:4px;padding:3px 7px}.diff-simple{display:grid;grid-template-columns:1fr 1fr;gap:10px}.diff-simple>div{border:1px solid var(--line);border-radius:8px;padding:10px 12px;display:grid;align-content:start;gap:4px;background:var(--surface)}.diff-simple span,.diff-simple small{font-size:12px;color:var(--muted)}.diff-simple strong{font-size:13px;font-weight:600}ul{padding-left:17px;font-size:13px;line-height:1.8}.diff-caution{display:flex;gap:8px;align-items:start;font-size:12px}.diff-caution .icon{color:var(--warning);width:16px;height:16px;margin-top:3px}.diff-raw{border-top:1px solid var(--line);padding-top:8px}.diff-raw summary{display:flex;align-items:center;justify-content:space-between;font-size:12px;color:var(--muted);cursor:pointer;list-style:none}.diff-raw[open] summary .icon{transform:rotate(180deg)}pre{white-space:pre-wrap;overflow-wrap:anywhere;font:12px/1.8 var(--font-mono);padding:12px;background:var(--surface-raised);margin-top:10px}.diff-add{color:var(--success)}.plan-hash{font-size:12px;overflow-wrap:anywhere}.readback-checks{margin-top:10px}.control{justify-self:start;display:flex;gap:7px;align-items:center}
@media(max-width:760px){.diff-simple{grid-template-columns:1fr}.diff-intro{align-items:start;flex-wrap:wrap}}
</style>
