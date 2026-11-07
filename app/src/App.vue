<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue'
import TopologyPane, { type DiscoveryRecord, type LldpNeighbor } from './TopologyPane.vue'
import logo from '../src-tauri/icons/icon.png'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import TerminalPane from './TerminalPane.vue'
import ToolDock from './ToolDock.vue'
import DeviceGlyph from './DeviceGlyph.vue'
import WorkbenchIcon from './WorkbenchIcon.vue'
import ConfigPlanDiff from './ConfigPlanDiff.vue'
import { namedRole, readRoles, roleLabels, type DeviceRole } from './device-presentation'
import { call, desktop, errorText, type Device, type Backup, type Task, type ConfigPlan } from './api'

const devices=ref<Device[]>([]), selectedId=ref(''), error=ref(''), busy=ref(''), runtime=ref(desktop?'正在检查执行程序…':'浏览器预览 · 设备功能不可用')
const selected=computed(()=>devices.value.find(d=>d.id===selectedId.value))
const customWindowsChrome=desktop && typeof navigator!=='undefined' && /Windows NT/i.test(navigator.userAgent)
async function windowAction(action:'minimize'|'maximize'|'close'){
  if(!customWindowsChrome)return
  try{
    const window=getCurrentWindow()
    if(action==='minimize')await window.minimize()
    else if(action==='maximize')await window.toggleMaximize()
    else await window.close()
  }catch(error){reportError(error)}
}
const vendorNames:Record<string,string>={huawei:'华为',ruijie:'锐捷',h3c:'新华三',cisco_ios:'思科 IOS',cisco_xe:'思科 IOS-XE',maipu:'迈普',raisecom_ros:'瑞斯康达 ROS',raisecom_roap:'瑞斯康达 ROAP',huawei_olt:'华为 OLT',huawei_smartax:'华为 SmartAX',zte_zxros:'中兴 ZXROS'}
const displayRoles=ref(readRoles()), closedGroups=ref<Record<string,boolean>>({})
const roleFor=(d:Device)=>displayRoles.value[d.id]||namedRole(d.name)
function saveRole(id:string,role:DeviceRole){if(!Object.prototype.hasOwnProperty.call(roleLabels,role))return;displayRoles.value={...displayRoles.value,[id]:role};try{localStorage.setItem('yigou:device-roles',JSON.stringify(displayRoles.value))}catch{/* Display preference remains in memory. */}}
const writableVendors=new Set(['huawei','ruijie','h3c'])
const backupVendors=new Set(['huawei','ruijie','h3c'])
// Mirrors worker DRIVERS query whitelist per platform.
const vendorQueries:Record<string,string[]>={huawei:['version','ports','config','neighbors','startup'],ruijie:['version','ports','config','neighbors','startup'],h3c:['version','ports','config','neighbors','startup'],cisco_ios:['version','ports','config','neighbors','startup'],cisco_xe:['version','ports','config','neighbors','startup'],maipu:['version','config','startup'],raisecom_ros:['version','config','startup'],huawei_olt:['version','config'],huawei_smartax:['version','config'],raisecom_roap:['version'],zte_zxros:['version']}
const vendorTier:Record<string,string>={huawei:'读写 · 备份与自动配置已仿真验收',ruijie:'读写 · 备份与自动配置已仿真验收',h3c:'读写 · 备份与自动配置已仿真验收',cisco_ios:'只读 · 自动写入未验收',cisco_xe:'只读 · 自动写入未验收',maipu:'只读 · 备份与自动写入未开放',raisecom_ros:'只读 · 备份与自动写入未开放',raisecom_roap:'只读 · 备份与自动写入未开放',huawei_olt:'只读 · 备份与自动写入未开放',huawei_smartax:'只读 · 备份与自动写入未开放',zte_zxros:'只读 · 备份与自动写入未开放'}
const vendorHasQuery=(vendor:string,query:string)=>(vendorQueries[vendor]||[]).includes(query)
const canBackup=(vendor:string)=>backupVendors.has(vendor)
const canWrite=(vendor:string)=>writableVendors.has(vendor)
const knownVendor=(vendor:string)=>vendorNames[vendor]||vendor
const addOpen=ref(false), modelOpen=ref(false), trustOpen=ref(false)
const newDevice=ref({name:'',host:'',port:22,vendor:'huawei',username:'',auto_backup:true,backup_minutes:60})
const credential=ref({kind:'password',secret:'',key_path:''})
const model=ref({base_url:'',model:'',api_key:'',key_configured:false})
const probe=ref<{algorithm:string;key:string;fingerprint:string}>(), confirmedFingerprint=ref('')
const prompt=ref(''), plan=ref(false), aiBusy=ref(false), aiDeviceId=ref('')
const autonomous=ref(false), autonomyConsent=ref(false), safetyApi=ref(false)
function selectPlanMode(checked:boolean){plan.value=checked;if(checked){autonomous.value=false;autonomyConsent.value=false}}
function selectAutonomyMode(checked:boolean){autonomous.value=checked;autonomyConsent.value=false;if(checked)plan.value=false}
type ReviewedPlan=ConfigPlan
const pendingPlan=ref<ReviewedPlan|null>(null), approvalOpen=ref(false)
const approval=ref({reviewed:false,backup_ack:false,rollback_ack:false,phrase:''})
const approvalReady=computed(()=>approval.value.reviewed&&approval.value.backup_ack&&approval.value.rollback_ack&&approval.value.phrase==='确认执行')
const workspaceView=ref<'ai'|'topology'>('ai'), deviceSearch=ref(''), sidebarCollapsed=ref(false)
const themeSetting=ref<'dark'|'light'|'system'>('light'), osDark=ref(true), motionSetting=ref<'immersive'|'standard'|'static'>('immersive'), prefersReduced=ref(false)
const resolvedTheme=computed(() => themeSetting.value==='system'?(osDark.value?'dark':'light'):themeSetting.value)
const effectiveMotion=computed(() => prefersReduced.value?'static':motionSetting.value)
const visibleDevices=computed(()=>devices.value.filter(d=>(d.name+' '+d.host+' '+(vendorNames[d.vendor]||d.vendor)).toLowerCase().includes(deviceSearch.value.trim().toLowerCase())))
const deviceGroups=computed(()=>[{id:'switch',label:'交换机',roles:['core','access','switch']},{id:'router',label:'路由器',roles:['router']},{id:'firewall',label:'防火墙',roles:['firewall']},{id:'unknown',label:'未分类设备',roles:['unknown']}].map(group=>({...group,devices:visibleDevices.value.filter(d=>group.roles.includes(roleFor(d)))})).filter(group=>group.devices.length))
const linkedNeighbors=ref<Record<string,DiscoveryRecord>>({}), aiPhase=ref<'idle'|'request'|'tool'|'complete'|'error'>('idle')
const mobileDevicesOpen=ref(false), contextOpen=ref(false), helpOpen=ref(false), terminalExpanded=ref(false)
const dockHeight=ref(0)
let releaseDockDrag:(()=>void)|undefined
const disposers:(()=>void)[]=[]
let disposed=false,queryResetEpoch=0,deviceRefresh=0,taskRefresh=0
const sessionEpochs=new Map<string,number>(),historyRefresh=new Map<string,number>()
function invalidateDeviceSession(id:string){
  sessionEpochs.set(id,(sessionEpochs.get(id)||0)+1)
  for(const message of messages.value)if(message.device===id&&message.configPlan?.status==='pending')message.configPlan.status='invalidated'
  if(pendingPlan.value?.device_id===id){approvalOpen.value=false;pendingPlan.value=null;approval.value.phrase=''}
}
type ConfigEvidence={status:string;phase?:string;write_attempted?:boolean;before_backup?:unknown;after_backup?:unknown;save_status?:string;attempted_commands?:number;confirmed_commands?:number;request?:{commands?:string[]};checks?:{query:string;passed:boolean}[];backup_error?:string;error?:string}
const configEvidence=ref<Record<string,ConfigEvidence>>({})
const recentConfigTask=computed(()=>tasks.value.find(t=>t.device_id===selectedId.value&&t.operation==='configure')||null)
const configView=computed<ConfigEvidence|undefined>(()=>{
  const direct=configEvidence.value[selectedId.value]
  if(direct)return direct
  const result=recentConfigTask.value?.result
  return result&&typeof result==='object'&&'status' in result ? result as ConfigEvidence : undefined
})
const configStages=[{label:'配置前备份',detail:'加密快照'},{label:'命令下发',detail:'真实设备执行'},{label:'回读核验',detail:'配置核对'},{label:'后置备份',detail:'加密归档'}]
function configStepState(index:number):'waiting'|'active'|'done'|'failed'|'unproven'{
  const record=configView.value
  if(!record)return 'waiting'
  if(index===0){
    if(record.write_attempted===true)return 'done' // Rust verifies the pre-backup before attempting writes.
    if(record.status==='running'&&record.phase==='backup')return 'active'
    return record.before_backup?'unproven':'waiting'
  }
  if(index===1){
    const total=record.request?.commands?.length||0
    if(total>0&&record.confirmed_commands===total)return 'done'
    if(record.status==='running'&&record.phase==='sending')return 'active'
    if(record.status==='partial')return 'failed'
    return record.write_attempted?'unproven':'waiting'
  }
  if(index===2){
    const checks=record.checks||[]
    if(checks.some(check=>check.passed===false))return 'failed'
    if(['succeeded','applied_backup_failed'].includes(record.status)&&checks.some(check=>check.query==='config')&&checks.every(check=>check.passed===true))return 'done'
    if(record.status==='running'&&record.phase==='finalizing')return 'active'
    return record.write_attempted?'unproven':'waiting'
  }
  if(record.after_backup)return 'done'
  if(record.backup_error)return 'failed'
  if(record.status==='running'&&record.phase==='finalizing')return 'active'
  return record.write_attempted?'unproven':'waiting'
}
const latestStatus=computed(()=>configView.value?.status||'暂无执行任务')
const selectedEvidence=computed(()=>linkedNeighbors.value[selectedId.value])
function resizeDock(event:PointerEvent){
  if(event.button!==0)return
  event.preventDefault()
  releaseDockDrag?.()
  const y=event.clientY,start=(event.currentTarget as HTMLElement).parentElement?.getBoundingClientRect().height||192
  const move=(e:PointerEvent)=>{dockHeight.value=Math.max(146,Math.min(Math.min(520,Math.floor(window.innerHeight*.68)),start+y-e.clientY))}
  const end=()=>{window.removeEventListener('pointermove',move);window.removeEventListener('pointerup',end);releaseDockDrag=undefined}
  window.addEventListener('pointermove',move)
  window.addEventListener('pointerup',end,{once:true})
  releaseDockDrag=end
}
function stepDock(delta:number){dockHeight.value=Math.max(146,Math.min(520,(dockHeight.value||document.querySelector('.tool-dock')?.getBoundingClientRect().height||192)+delta))}
type ToolEvidence={ok:boolean;query?:string;observed_at?:string;error?:string;data?:{command?:string;output?:string;truncated?:boolean}}
type Message={role:'user'|'assistant';text:string;device:string;request?:string;pending?:boolean;failed?:boolean;tools?:string[];evidence?:ToolEvidence[];configPlan?:ReviewedPlan}
const messages=ref<Message[]>([])
const visibleMessages=computed(()=>messages.value.filter(m=>m.device===selectedId.value))
const results=ref<Record<string,{title:string;text:string}>>({}), panel=ref('terminal')
const output=computed(()=>results.value[selectedId.value]?.text||''), resultTitle=computed(()=>results.value[selectedId.value]?.title||'执行结果')
const historyByDevice=ref<Record<string,Backup[]>>({}), tasks=ref<Task[]>([])
const histories=computed(()=>historyByDevice.value[selectedId.value]||[])
const trustDevice=ref<Device>(), terminalOpening=ref(false), closingTerminals=ref(new Set<string>())
const terminals=ref<{session:string;device:Device}[]>([]), activeTerminal=ref('')
const activeMessages=computed(()=>visibleMessages.value.length)
const latestPlan=computed(()=>[...visibleMessages.value].reverse().find(m=>m.configPlan)?.configPlan||null)
const selectedTasks=computed(()=>tasks.value.filter(t=>t.device_id===selectedId.value))
const assistantMode=computed({get:()=>autonomous.value?'autonomous':plan.value?'plan':'analysis',set:(value:string)=>{selectAutonomyMode(value==='autonomous');selectPlanMode(value==='plan')}})
const sendLabel=computed(()=>aiBusy.value?(aiDeviceId.value===selectedId.value?'处理中…':'其他设备处理中'):autonomous.value?'授权并执行':plan.value?'生成方案':'开始检查')
const commonActions=[{id:'offline',label:'网络上不了网',prompt:'当前网络无法上网，请查询设备状态、接口和邻居证据，分析原因，先检查不修改。'},{id:'slow',label:'网络很慢',prompt:'当前网络很慢，请基于设备实际查询结果检查接口和链路，说明证据与限制，先检查不修改。'},{id:'neighbors',label:'查看设备连接',prompt:'请查询当前设备的 LLDP 邻居，结合实际端口说明连接关系和证据。'},{id:'plan',label:'新增网络',prompt:'我想新增一个网络。请先询问网络编号、名称、接口及影响范围，信息完整后生成待审批方案，不要直接执行。'}]
function promptKey(event:KeyboardEvent){if(event.key!=='Enter'||event.shiftKey||event.altKey||event.isComposing||event.keyCode===229)return;event.preventDefault();void sendChat()}
function suggestTask(id:string){const item=commonActions.find(a=>a.id===id);if(!item)return;assistantMode.value=id==='plan'?'plan':'analysis';prompt.value=item.prompt;pickView('ai');window.setTimeout(()=>document.getElementById('prompt')?.focus(),0)}
let unlisten:UnlistenFn|undefined

async function refresh() {const epoch=++deviceRefresh,list=await call<Device[]>('device_list');if(disposed||epoch!==deviceRefresh)return;devices.value=list;if(!selectedId.value&&list.length)selectedId.value=list[0].id}
async function refreshTasks(){if(!desktop||disposed)return;const epoch=++taskRefresh,list=await call<Task[]>('task_status',{taskId:null});if(!disposed&&epoch===taskRefresh)tasks.value=list}
function showResult(id:string,title:string,text:string){results.value[id]={title,text};if(selectedId.value===id)panel.value='tasks'}
function reportError(e:unknown,id?:string){error.value=(id?`${devices.value.find(d=>d.id===id)?.name||id}：`:'')+errorText(e)}
async function action(label:string, work:()=>Promise<void>, owner=selectedId.value) {
  if(busy.value)return
  busy.value=label;error.value=''
  try{await work()}catch(e){reportError(e,owner)}finally{busy.value='';try{await refreshTasks()}catch{/* The primary error remains visible. */}}
}
async function addDevice() {await action('添加设备',async()=>{
  const added=await call<Device>('device_add',{device:newDevice.value,credential:{...credential.value,key_path:credential.value.key_path||null}})
  credential.value.secret='';credential.value.key_path='';addOpen.value=false;await refresh();selectDevice(added.id);contextOpen.value=true
})}
function closeAdd(){credential.value.secret='';credential.value.key_path='';addOpen.value=false}
function onVendorChange(){if(!canBackup(newDevice.value.vendor))newDevice.value.auto_backup=false}
async function connectDevice(){const d=selected.value;if(!d)return
  await action('连接设备',async()=>{
    invalidateDeviceSession(d.id)
    const result=await call<{backup?:{status:string;error?:string}}> ('device_connect',{deviceId:d.id});await refresh()
    showResult(d.id,'连接结果',JSON.stringify(result,null,2))
    if(result.backup?.status==='failed')error.value=`${d.name}：SSH 已连接，但自动备份失败：${result.backup.error}`
  })
}
async function disconnect(){const d=selected.value;if(!d)return;await action('断开连接',async()=>{invalidateDeviceSession(d.id);await call('device_disconnect',{deviceId:d.id});terminals.value=terminals.value.filter(t=>t.device.id!==d.id);await refresh()})}
async function runQuery(query:string){const d=selected.value;if(!d||!d.connected||!vendorHasQuery(d.vendor,query))return;const epoch=sessionEpochs.get(d.id)||0,resetEpoch=queryResetEpoch;await action('查询设备',async()=>{
  const result=await call<{output:string;command:string;neighbors?:LldpNeighbor[];parsed?:boolean;raw?:string;note?:string}> (query==='neighbors'?'device_discover':'device_query',{deviceId:d.id,query})
  if(disposed||epoch!==(sessionEpochs.get(d.id)||0)||resetEpoch!==queryResetEpoch)return
  if(query==='neighbors'){ const neighbors=Array.isArray(result.neighbors)?result.neighbors:[];linkedNeighbors.value={...linkedNeighbors.value,[d.id]:{neighbors,parsed:result.parsed===true&&neighbors.length>0,raw:result.raw||result.output,note:result.note,checkedAt:new Date().toISOString()}} }
  showResult(d.id,result.command,query==='neighbors'?JSON.stringify(result,null,2):result.output)
})}
async function backup(){const d=selected.value;if(!d)return;await action('备份配置',async()=>{
  const result=await call<Backup>('device_backup',{deviceId:d.id});showResult(d.id,'配置已加密备份并校验',JSON.stringify(result,null,2));await loadHistory(d.id);if(selectedId.value===d.id)panel.value='history'
})}
async function loadHistory(id:string){const epoch=(historyRefresh.get(id)||0)+1;historyRefresh.set(id,epoch);const rows=await call<Backup[]>('backup_history',{deviceId:id});if(!disposed&&historyRefresh.get(id)===epoch)historyByDevice.value[id]=rows}
async function showHistory(){const id=selectedId.value;if(!id)return;panel.value='history';await action('读取备份历史',()=>loadHistory(id),id)}
async function reindexHistory(){const d=selected.value;if(!d)return;await action('重建备份记录',async()=>{const result=await call<{recovered_count:number}>('backup_reindex',{deviceId:d.id});await loadHistory(d.id);showResult(d.id,'备份记录重建完成',`已恢复 ${result.recovered_count} 条通过完整性验证的备份记录。`);if(selectedId.value===d.id&&historyByDevice.value[d.id]?.length)panel.value='history'},d.id)}
async function verifyBackup(record:Backup){await action('校验备份',async()=>{const result=await call<{preview:string;sha256:string}>('backup_verify',{deviceId:record.device_id,backupId:record.id});showResult(record.device_id,'备份解密与完整性校验通过 · 脱敏预览',result.preview)},record.device_id)}
async function probeKey(){const d=selected.value;if(!d)return;await action('读取 SSH 指纹',async()=>{const result=await call<typeof probe.value>('host_key_probe',{deviceId:d.id});probe.value=result;trustDevice.value={...d};confirmedFingerprint.value='';trustOpen.value=true},d.id)}
async function trustKey(){const d=trustDevice.value;if(!d||!probe.value)return;await action('登记主机密钥',async()=>{invalidateDeviceSession(d.id);await call('host_key_trust',{deviceId:d.id,...probe.value,confirmedFingerprint:confirmedFingerprint.value.trim()});trustOpen.value=false;terminals.value=terminals.value.filter(t=>t.device.id!==d.id);await refresh()},d.id)}
async function saveModel(){await action('保存模型配置',async()=>{const result=await call<typeof model.value>('model_config_save',{baseUrl:model.value.base_url,model:model.value.model,apiKey:model.value.api_key});model.value={...result,api_key:''};modelOpen.value=false})}
async function sendChat(){const d=selected.value;if(!d||!d.connected||busy.value||!prompt.value.trim()||aiBusy.value||!safetyApi.value)return
  const isAuto=autonomous.value,isPlan=plan.value
  if(isAuto&&(!autonomyConsent.value||!canWrite(d.vendor)))return
  const text=prompt.value,request=crypto.randomUUID(),epoch=sessionEpochs.get(d.id)||0,resetEpoch=queryResetEpoch
  prompt.value='';aiBusy.value=true;aiDeviceId.value=d.id;error.value='';aiPhase.value='request'
  autonomous.value=false;autonomyConsent.value=false // Per-request authorization never carries over.
  messages.value.push({role:'user',text,device:d.id},{role:'assistant',text:'',device:d.id,request,pending:true,tools:[]})
  try{
    const command=isAuto?'ai_autonomous':isPlan?'ai_plan':'ai_chat'
    const result=await call<{text:string;plan?:ReviewedPlan|null;executed?:boolean|null;configuration?:ConfigEvidence|null}>(command,{
      deviceId:d.id,prompt:text,requestId:request,...(isAuto?{consent:true}:{})
    })
    const message=messages.value.find(m=>m.request===request)
    if(!message||disposed)return
    if(epoch!==(sessionEpochs.get(d.id)||0)||resetEpoch!==queryResetEpoch){message.text='设备查询会话已变化，本轮返回不再作为当前会话的计划或执行依据；请查看实际执行记录。';message.failed=true;aiPhase.value='error';return}
    message.text=(isPlan?'配置计划预览 · 未执行\n\n':isAuto?'自主模式 · 以下结果以 Rust 后端记录为准\n\n':'')+result.text
    if(result.plan){
      if(!result.plan.id||!result.plan.request_hash||result.plan.device_id!==d.id||!Array.isArray(result.plan.config?.commands))throw Error('配置计划协议不匹配，请使用具备审批与受限自主执行接口的 Rust 后端。')
      message.configPlan=result.plan
    }
    if(result.configuration){
      configEvidence.value={...configEvidence.value,[d.id]:result.configuration}
      showResult(d.id,'自主配置执行记录 · 实际回读与备份',JSON.stringify(result.configuration,null,2))
      void loadHistory(d.id).catch(()=>{})
    }
  }
  catch(e){const message=messages.value.find(m=>m.request===request)!;message.text=errorText(e);message.failed=true;aiPhase.value='error'}
  finally{const message=messages.value.find(m=>m.request===request);if(message)message.pending=false;aiBusy.value=false;if(aiPhase.value!=='error')aiPhase.value='complete';await refreshTasks().catch(()=>{})}
}
function reviewPlan(candidate:ReviewedPlan){
  if(candidate.status!=='pending'||candidate.device_id!==selectedId.value){error.value='计划设备已变化或审批已结束，请重新生成计划。';return}
  pendingPlan.value=candidate
  approval.value={reviewed:false,backup_ack:false,rollback_ack:false,phrase:''}
  approvalOpen.value=true
}
function closeApproval(){if(busy.value)return;approvalOpen.value=false;approval.value.phrase=''}
async function executeApproved(){
  const p=pendingPlan.value
  if(!p||!safetyApi.value||!approvalReady.value||p.device_id!==selectedId.value)return
  await action('配置审批与执行',async()=>{
    const outcome=await call<ConfigEvidence>('config_execute_approved',{
      deviceId:p.device_id,planId:p.id,reviewedHash:p.request_hash,approval:{...approval.value},
    })
    p.status='consumed'
    approvalOpen.value=false
    approval.value.phrase=''
    configEvidence.value={...configEvidence.value,[p.device_id]:outcome}
    showResult(p.device_id,'配置执行记录 · 以回读和备份为准',JSON.stringify(outcome,null,2))
    await loadHistory(p.device_id)
  },p.device_id)
}
async function openTerminal(){const d=selected.value;if(!d||!d.host_key_trusted||terminalOpening.value||busy.value)return
  const epoch=sessionEpochs.get(d.id)||0
  terminalOpening.value=true
  try{
    const t=await call<{session:string}>('terminal_open',{deviceId:d.id})
    if(disposed||epoch!==(sessionEpochs.get(d.id)||0)){await call('terminal_close',{session:t.session});return}
    terminals.value.push({session:t.session,device:{...d}})
    if(selectedId.value===d.id){activeTerminal.value=t.session;panel.value='terminal'}
  }
  catch(e){if(!disposed)reportError(e,d.id)}finally{if(!disposed){terminalOpening.value=false;void refreshTasks().catch(()=>{})}}
}
async function closeTerminal(session:string){if(closingTerminals.value.has(session))return;closingTerminals.value.add(session)
  try{await call('terminal_close',{session});terminals.value=terminals.value.filter(t=>t.session!==session);if(activeTerminal.value===session)activeTerminal.value=terminals.value.at(-1)?.session||''}
  catch(e){reportError(e,terminals.value.find(t=>t.session===session)?.device.id)}finally{closingTerminals.value.delete(session);void refreshTasks().catch(()=>{})}
}
function selectDevice(id:string){selectedId.value=id;autonomous.value=false;autonomyConsent.value=false;mobileDevicesOpen.value=false;diffBefore.value='';diffAfter.value='';diffResult.value=null;diffEpoch++;if(desktop&&['history','diff'].includes(panel.value))void loadHistory(id).catch(e=>reportError(e,id))}
function showTaskResult(task:Task){if(task.device_id){selectDevice(task.device_id);showResult(task.device_id,'配置执行记录',JSON.stringify(task.result,null,2))}}
onMounted(async()=>{
  if(!desktop)return
  try{
    unlisten=await listen<Record<string,unknown>>('yigou:event',({payload:e})=>{
      if(disposed)return
      // The durable configuration request is the plan ID. Only its trusted
      // source_request_id ties execution events back to the originating chat.
      const origin=e.kind?.toString().startsWith('ai_config_')&&typeof e.source_request_id==='string'
        ? e.source_request_id : e.request_id
      const message=messages.value.find(m=>m.request===origin&&(!e.device_id||m.device===e.device_id))
      if(e.kind==='ai_delta'&&message){message.text+=String(e.text);aiPhase.value='tool'}
      if(e.kind==='ai_tool_start'&&message){message.tools?.push(`查询 ${String(e.query)}…`);aiPhase.value='tool'}
      if(e.kind==='ai_tool_result'&&message){const result=e.result as ToolEvidence;message.tools?.push(`${result.query||'查询'}：${result.ok?'已返回设备证据':result.error}`);message.evidence??=[];message.evidence.push(result)}
      if(e.kind==='sessions_reset'){queryResetEpoch++;for(const device of devices.value)invalidateDeviceSession(device.id);linkedNeighbors.value={};void refresh().catch(()=>{});error.value='设备查询会话已断开，请重新连接查询。'}
      if(e.kind==='task'){void refreshTasks().catch(()=>{});if(e.status!=='running')void refresh().catch(()=>{})}
      if(e.kind==='ai_config_start'){
        const id=String(e.device_id)
        configEvidence.value={...configEvidence.value,[id]:{status:'running',phase:'backup',write_attempted:false}}
      }
      if(e.kind==='ai_config_phase'){
        const id=String(e.device_id)
        const previous=configEvidence.value[id]||{status:'running'}
        const phase=String(e.phase||'')
        configEvidence.value={...configEvidence.value,[id]:{...previous,phase,write_attempted:phase==='sending'||phase==='finalizing'||previous.write_attempted||false,before_backup:e.before_backup||previous.before_backup}}
      }
      if(e.kind==='ai_config_result'){
        const id=String(e.device_id)
        const result=e.result as ConfigEvidence
        configEvidence.value={...configEvidence.value,[id]:result}
        message?.tools?.push(`配置执行：${result.status}`)
        showResult(id,'AI 配置执行结果',JSON.stringify(result,null,2))
      }
    })
    if(disposed){unlisten();return}
    const status=await call<{version:string;ai_mode?:string}>('system_status');safetyApi.value=status.ai_mode==='optional_autonomous';runtime.value=`本地核心 ${status.version}`
    await refresh();await refreshTasks()
    const saved=await call<Omit<typeof model.value,'api_key'>|null>('model_config_get');if(saved)model.value={...saved,api_key:''}
  }catch(e){runtime.value='本地核心不可用';error.value=errorText(e)}
})
onBeforeUnmount(()=>{disposed=true;unlisten?.();releaseDockDrag?.();disposers.forEach(dispose=>dispose())})

function pickView(view:'ai'|'topology'|'terminal'){
  terminalExpanded.value=false
  if(workspaceView.value===view)return
  if(view==='terminal'){panel.value='terminal';terminalExpanded.value=true;return}
  workspaceView.value=view
}
function openFromTopology(id:string){selectDevice(id);contextOpen.value=true}
function setTheme(mode:'dark'|'light'|'system'){themeSetting.value=mode;try{localStorage.setItem('yigou:theme',mode)}catch{/*storage optional*/}}
function setMotion(mode:'immersive'|'standard'|'static'){motionSetting.value=mode;try{localStorage.setItem('yigou:motion',mode)}catch{/*storage optional*/}}
function focusDeviceSearch(){mobileDevicesOpen.value=true;sidebarCollapsed.value=false;window.setTimeout(()=>document.getElementById('device-search')?.focus(),0)}
onMounted(()=>{
  const shortcut=(event:KeyboardEvent)=>{
    if(event.key==='Escape'){mobileDevicesOpen.value=false;contextOpen.value=false;helpOpen.value=false}
    if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){
      event.preventDefault();focusDeviceSearch()
    }
  }
  window.addEventListener('keydown',shortcut)
  disposers.push(()=>window.removeEventListener('keydown',shortcut))
  let prefersDark:MediaQueryList|undefined
  let prefersMotion:MediaQueryList|undefined
  try{
    const theme=localStorage.getItem('yigou:theme')
    if(theme==='dark'||theme==='light'||theme==='system')themeSetting.value=theme
    const motion=localStorage.getItem('yigou:motion')
    if(motion==='immersive'||motion==='standard'||motion==='static')motionSetting.value=motion
  }catch{/*storage unavailable*/}
  if(typeof matchMedia==='function'){
    prefersDark=matchMedia('(prefers-color-scheme:dark)')
    prefersMotion=matchMedia('(prefers-reduced-motion:reduce)')
    osDark.value=prefersDark.matches
    prefersReduced.value=prefersMotion.matches
    const onDark=(event:MediaQueryListEvent)=>{osDark.value=event.matches}
    const onReduced=(event:MediaQueryListEvent)=>{prefersReduced.value=event.matches}
    prefersDark.addEventListener('change',onDark)
    prefersMotion.addEventListener('change',onReduced)
    disposers.push(()=>{prefersDark?.removeEventListener('change',onDark);prefersMotion?.removeEventListener('change',onReduced)})
  }
})

function newConversation(){if(aiBusy.value)return;terminalExpanded.value=false;assistantMode.value='analysis';messages.value=messages.value.filter(m=>m.device!==selectedId.value);prompt.value='';aiPhase.value='idle';pickView('ai')}
function selectPanel(value:string){panel.value=value;if(value==='history')void showHistory();if(value==='diff')void openDiff()}
const diffBefore=ref(''),diffAfter=ref('')
let diffEpoch=0
const diffResult=ref<{summary:string;lines:{kind:'same'|'added'|'removed';text:string}[]}|null>(null)
async function openDiff(){panel.value='diff';const id=selectedId.value;if(id)await action('读取备份历史',()=>loadHistory(id),id)}
async function compareBackups(){
  const id=selectedId.value,before=diffBefore.value,after=diffAfter.value,epoch=++diffEpoch
  if(!id||!before||!after||before===after)return
  diffResult.value=null
  await action('比较配置',async()=>{
    const left=await call<{preview:string}>('backup_verify',{deviceId:id,backupId:before})
    const right=await call<{preview:string}>('backup_verify',{deviceId:id,backupId:after})
    if(id!==selectedId.value||epoch!==diffEpoch||before!==diffBefore.value||after!==diffAfter.value)return
    const a=left.preview.replace(/\r\n/g,'\n').split('\n'),b=right.preview.replace(/\r\n/g,'\n').split('\n')
    let start=0,end=0
    while(start<a.length&&start<b.length&&a[start]===b[start])start++
    while(end<a.length-start&&end<b.length-start&&a[a.length-1-end]===b[b.length-1-end])end++
    const removed=a.slice(start,a.length-end),added=b.slice(start,b.length-end)
    diffResult.value={summary:!removed.length&&!added.length?'两份已校验的脱敏配置文本一致。':`从第 ${start+1} 行开始的变化区段：旧文本 ${removed.length} 行，新文本 ${added.length} 行。中间相同行保留在变化区段中，请展开核对具体命令。`,lines:[...a.slice(Math.max(0,start-2),start).map(text=>({kind:'same' as const,text})),...removed.map(text=>({kind:'removed' as const,text})),...added.map(text=>({kind:'added' as const,text})),...b.slice(b.length-end,b.length-end+2).map(text=>({kind:'same' as const,text}))]}
  },id)
}

</script>

<template>
  <div class="app-shell" :data-theme="resolvedTheme" :data-motion="effectiveMotion" :class="{'context-visible':contextOpen,'terminal-expanded':terminalExpanded}" :data-activity="busy || (aiBusy?'处理模型请求':configView?.status==='running'?'配置任务执行中':'')">
    <header class="app-header" :data-tauri-drag-region="customWindowsChrome?true:undefined">
      <div class="brand" aria-label="意构桌面客户端">
        <span class="brand-emblem"><img :src="logo" alt="意构标志" width="34" height="34"/></span>
        <div class="brand-wordmark"><strong>意构</strong><span>YIGOU / NETWORK STUDIO</span></div>
      </div>
      <button class="workspace-picker" @click="focusDeviceSearch">本地网络 <WorkbenchIcon name="chevron-down"/></button>
      <button class="command-search" @click="focusDeviceSearch" title="查找设备"><WorkbenchIcon name="search"/><span>查找设备或地址</span><kbd>Ctrl K</kbd></button>
      <div class="header-spacer" :data-tauri-drag-region="customWindowsChrome?true:undefined"></div>
      <span class="runtime" :title="runtime">{{runtime}}</span>
      <button class="guide-entry" @click="helpOpen=true">使用帮助</button>
      <div class="theme-controls" role="group" aria-label="颜色主题">
        <button aria-label="深色主题" title="深色" :aria-pressed="themeSetting==='dark'" :class="{active:themeSetting==='dark'}" @click="setTheme('dark')"><WorkbenchIcon name="moon"/></button>
        <button aria-label="浅色主题" title="浅色" :aria-pressed="themeSetting==='light'" :class="{active:themeSetting==='light'}" @click="setTheme('light')"><WorkbenchIcon name="sun"/></button>
        <button aria-label="跟随系统主题" title="跟随系统" :aria-pressed="themeSetting==='system'" :class="{active:themeSetting==='system'}" @click="setTheme('system')"><WorkbenchIcon name="device-desktop"/></button>
      </div>
      <button class="model-settings-btn" aria-label="模型设置" title="模型设置" :disabled="!desktop||!!busy" @click="modelOpen=true"><WorkbenchIcon name="settings-2"/></button>
      <div v-if="customWindowsChrome" class="window-actions" role="group" aria-label="Windows 窗口控制"><button title="最小化" aria-label="最小化窗口" @click="windowAction('minimize')">−</button><button title="最大化或还原" aria-label="最大化或还原窗口" @click="windowAction('maximize')">▢</button><button class="window-close" title="关闭" aria-label="关闭窗口" @click="windowAction('close')">×</button></div>
    </header>
    <div v-if="error" class="error-banner" role="alert"><span>{{error}}</span><button aria-label="关闭错误提示" @click="error=''">×</button></div>
    <div class="workspace">
      <button v-if="mobileDevicesOpen" class="device-backdrop" aria-label="关闭设备列表" @click="mobileDevicesOpen=false"></button>
      <aside class="devices" :class="{collapsed:sidebarCollapsed,'mobile-open':mobileDevicesOpen}">
        <div class="sidebar-topline">
          <strong>网络设备</strong>
          <button aria-label="折叠或展开侧栏" title="折叠或展开侧栏" @click="sidebarCollapsed=!sidebarCollapsed"><WorkbenchIcon :name="sidebarCollapsed?'layout-sidebar-left-expand':'layout-sidebar-left-collapse'"/></button>
        </div>
        <div class="sidebar-list-head"><span class="sidebar-copy">设备清单</span><span>{{devices.length.toString().padStart(2,'0')}}</span></div>
        <div class="device-search"><WorkbenchIcon name="search"/><label class="sr-only" for="device-search">搜索设备</label><input id="device-search" v-model="deviceSearch" type="search" autocomplete="off" placeholder="搜索设备或地址"/></div>
        <nav class="device-list" aria-label="设备列表">
          <section v-for="group in deviceGroups" :key="group.id" class="device-tree-group">
            <button class="device-group" :aria-expanded="!closedGroups[group.id]||sidebarCollapsed" @click="closedGroups[group.id]=!closedGroups[group.id]"><WorkbenchIcon :name="closedGroups[group.id]?'chevron-right':'chevron-down'"/><span class="sidebar-copy">{{group.label}}</span><span>{{group.devices.length}}</span></button>
            <div v-show="!closedGroups[group.id]||sidebarCollapsed" class="device-group-children">
              <button v-for="device in group.devices" :key="device.id" class="device" :class="{selected:device.id===selectedId}" :title="device.name+' · '+device.host" :aria-label="device.name+' '+knownVendor(device.vendor)+' '+device.host" :aria-pressed="device.id===selectedId" @click="selectDevice(device.id)">
                <DeviceGlyph :name="device.name" :role="roleFor(device)"/><span class="device-details"><strong>{{device.name}}</strong><small>{{device.host}}</small></span><i class="device-status" :class="{online:device.connected}" :aria-label="device.connected?'已连接':'未连接'"></i>
              </button>
            </div>
          </section>
          <div v-if="!devices.length" class="sidebar-empty"><strong>尚未添加设备</strong><span>先添加设备并核实主机指纹，再建立 SSH 连接。</span></div>
          <div v-else-if="!visibleDevices.length" class="sidebar-empty">未找到匹配的设备</div>
        </nav>
        <button class="add-button" :disabled="!desktop||!!busy" @click="addOpen=true"><WorkbenchIcon name="plus"/><span class="sidebar-copy">添加设备</span></button>
        <p class="sidebar-note"><WorkbenchIcon name="shield"/><span class="sidebar-copy">设备凭据保存在系统凭据库<br/><em>实际型号兼容性需验证</em></span></p>
      </aside>
      <main class="command-deck">
        <nav class="command-nav" aria-label="当前工作区">
          <div class="view-tabs" role="group" aria-label="工作视图"><button :aria-pressed="workspaceView==='ai'&&!terminalExpanded" :class="{active:workspaceView==='ai'&&!terminalExpanded}" @click="pickView('ai')"><WorkbenchIcon name="message-circle"/>问题处理</button><button :aria-pressed="workspaceView==='topology'&&!terminalExpanded" :class="{active:workspaceView==='topology'&&!terminalExpanded}" @click="pickView('topology')"><WorkbenchIcon name="topology-star-3"/>网络拓扑</button></div>
          <div class="nav-session"><button class="mobile-device-button" :aria-expanded="mobileDevicesOpen" @click="mobileDevicesOpen=!mobileDevicesOpen"><WorkbenchIcon name="server"/>设备</button><button class="nav-new" :disabled="!selected||aiBusy" @click="newConversation"><WorkbenchIcon name="plus"/><span>新对话</span></button><button class="header-context-btn" :aria-expanded="contextOpen" aria-label="查看当前设备详情" @click="contextOpen=!contextOpen"><WorkbenchIcon name="server"/><span>设备详情</span></button></div>
        </nav>
        <div class="main-content" :class="{'has-context':contextOpen,'is-topology':workspaceView==='topology'}">
          <div v-show="!terminalExpanded" class="central-stage">
            <Transition name="view-change" mode="out-in">
            <section v-if="workspaceView==='ai'" key="ai" class="assistant" :class="{'is-landing':!activeMessages}" aria-label="AI 助手">
              <header class="assistant-heading">
                <div class="heading-copy"><p class="session-kicker">当前设备 <span>/</span><strong>{{selected?.name||'尚未选择设备'}}</strong></p><h1>网络问题处理</h1><p>描述要检查的情况，或从常用操作中选择。</p></div>
                <div class="heading-device" :class="{connected:selected?.connected,working:!!busy||aiBusy}"><i class="heading-status-dot"></i><div><strong>{{selected?.name||'未选择设备'}}</strong><small>{{busy|| (aiBusy?'正在执行请求': selected?.connected?'查询已连接':'等待连接')}}</small></div></div>
              </header>
              <div v-if="aiDeviceId===selectedId&&(aiBusy||aiPhase==='error')" class="task-activity" :data-phase="aiPhase" role="status">
                <span class="task-activity-led"></span>
                <strong>{{aiPhase==='request'?'正在分析请求':aiPhase==='tool'?'正在获取设备证据':aiPhase==='error'?'请求失败':'执行结果已返回'}}</strong>
                <span>内容来自实际模型或设备调用</span>
              </div>
              <div class="messages" aria-live="polite">
                <article v-for="(message,index) in visibleMessages" :key="index" class="message" :class="[message.role,{failed:message.failed}]">
                  <div class="message-role"><span class="message-icon"><template v-if="message.role==='user'">我</template><WorkbenchIcon v-else name="message-circle"/></span>{{message.role==='user'?'你':'意构'}}<span v-if="message.pending" class="pending-label">处理中 <i class="dot-loader"></i></span></div>
                  <ul v-if="message.tools?.length" class="tool-events"><li v-for="(tool,n) in message.tools" :key="n">{{tool}}</li></ul>
                  <p>{{message.text || (message.pending?'等待模型回复…':'模型未返回文本')}}</p>
                  <details v-for="(evidence,n) in message.evidence" :key="n" class="evidence-detail">
                    <summary>查询证据：{{evidence.query||'设备'}} · {{evidence.ok?'已返回':'查询失败'}}</summary>
                    <div v-if="evidence.observed_at" class="evidence-time">采集于 {{evidence.observed_at}}</div>
                    <pre>{{evidence.ok?(evidence.data?.output||JSON.stringify(evidence.data,null,2)):(evidence.error||'未取得数据')}}</pre>
                    <small v-if="evidence.data?.truncated">设备结果已截断，不应当作完整配置。</small>
                  </details>
                  <section v-if="message.configPlan" class="reviewed-plan" aria-label="待审批的结构化配置计划">
                    <div class="reviewed-plan-head"><strong>待审批的配置计划</strong><span>{{message.configPlan.status==='pending'?'待审批 · 未执行':message.configPlan.status==='consumed'?'已提交 · 结果见执行记录':'计划已失效'}}</span></div>
                    <p>目标设备：{{devices.find(d=>d.id===message.configPlan?.device_id)?.name||'设备不可用'}} · 保存启动配置：{{message.configPlan.config.save?'是':'否'}}</p>
                    <details><summary>展开原始命令（{{message.configPlan.config.commands.length}} 条）</summary><pre>{{message.configPlan.config.commands.join('\n')}}</pre></details>
                    <div class="review-checks"><strong>回读条件</strong><p v-for="(check,j) in message.configPlan.config.checks" :key="j">{{check.query}}：包含 {{check.contains.join('、')||'—'}}；不包含 {{check.not_contains.join('、')||'—'}}</p></div>
                    <small>计划校验值：{{message.configPlan.request_hash}} · 有效期 15 分钟，绑定生成时设备会话。</small>
                    <button type="button" class="primary" :disabled="message.configPlan.status!=='pending'||selectedId!==message.configPlan.device_id||!selected?.connected||!!busy" @click="reviewPlan(message.configPlan)">审阅风险并审批</button>
                  </section>
                </article>
              </div>
              <div class="composer-wrap">
                <form class="composer" @submit.prevent="sendChat">
                  <label class="sr-only" for="prompt">描述网络任务</label>
                  <textarea id="prompt" v-model="prompt" rows="2" placeholder="例如：三楼会议室网络经常断开，帮我检查原因" :disabled="!selected?.connected||aiBusy||!safetyApi" @keydown="promptKey"/>
                  <div class="composer-footer">
                    <label class="sr-only" for="assistant-mode">助手工作模式</label><select id="assistant-mode" v-model="assistantMode" class="mode-select" :disabled="aiBusy||!safetyApi"><option value="analysis">先检查，不修改</option><option value="plan">先给我看方案</option><option value="autonomous" :disabled="!selected?.connected||!canWrite(selected?.vendor||'')">授权自主处理</option></select>
                    <button type="button" class="composer-target" @click="contextOpen=true"><WorkbenchIcon name="server"/><span>当前：<strong>{{selected?.name||'未选择设备'}}</strong></span></button><span class="spacer"></span>
                    <button class="primary send-button" type="submit" :disabled="!safetyApi||!selected?.connected||aiBusy||!!busy||!prompt.trim()||(autonomous&&!autonomyConsent)"><WorkbenchIcon name="arrow-up"/><span>{{sendLabel}}</span></button>
                  </div>
                  <label v-if="autonomous" class="autonomy-consent check"><input v-model="autonomyConsent" type="checkbox">我授权 AI 对当前设备执行本次低风险配置；必须先完成备份与回读核验，高风险操作转人工审批，不会自动回滚。</label>
                </form>
                <section class="common-actions" aria-label="常用操作"><div><strong>常用操作</strong><span>真实设备查询与计划</span></div><div class="common-actions-list"><button v-for="item in commonActions" :key="item.id" :disabled="!selected?.connected||aiBusy||!safetyApi" @click="suggestTask(item.id)">{{item.label}}<WorkbenchIcon name="arrow-right"/></button></div></section>
                <p v-if="desktop&&!safetyApi" class="mode-note compatibility-note">当前核心尚未提供新版安全审批能力，AI 请求暂不可用。</p><p v-else-if="!selected?.connected" class="mode-note connection-hint">{{selected?'核实 SSH 指纹并连接设备后开始。':'添加真实设备后开始。当前没有设备数据。'}} <button @click="contextOpen=true">设备详情 <WorkbenchIcon name="arrow-right"/></button></p>
                <p v-else class="mode-note">{{autonomous?'本次授权仅对当前设备、本轮请求生效。后端仍会校验命令、备份与回读条件。':plan?'先看影响和方案，由你决定是否继续。':'先了解问题，不修改任何设备。'}}</p>
              </div>
            </section>
            <TopologyPane v-else-if="workspaceView==='topology'" key="topology" :devices="devices" :selected-id="selectedId" :records="linkedNeighbors" :loading="busy==='查询设备'" :motion="effectiveMotion" :roles="displayRoles" :discovery-supported="vendorHasQuery(selected?.vendor||'','neighbors')" @select="openFromTopology" @discover="runQuery('neighbors')"/>

            </Transition>
          </div>

          <aside v-if="contextOpen" class="context-panel" aria-label="当前设备详情">
            <div class="panel-heading"><strong>当前设备</strong><button aria-label="关闭设备详情" @click="contextOpen=false"><WorkbenchIcon name="x"/></button></div>
            <template v-if="selected">
              <div class="device-visual"><DeviceGlyph :name="selected.name" :role="roleFor(selected)"/><span>{{roleLabels[roleFor(selected)]}}</span><i class="device-status" :class="{online:selected.connected}"/></div>
              <h2 class="context-name">{{selected.name}}</h2><p class="context-caption">{{knownVendor(selected.vendor)}} · {{selected.host}}</p><p class="context-status"><i class="device-status" :class="{online:selected.connected}"/>{{selected.connected?'查询已连接':'未连接'}}</p>
              <div class="context-buttons"><button class="control" @click="pickView('topology')"><WorkbenchIcon name="topology-star-3"/>查看连接位置</button><button class="control" :disabled="!selected.host_key_trusted||!!busy" @click="selected?.connected?disconnect():connectDevice()"><WorkbenchIcon name="plug-connected"/>{{selected.connected?'断开连接':'连接设备'}}</button></div>
              <button class="context-ask" :disabled="!selected.connected||aiBusy||!safetyApi" @click="pickView('ai');prompt='请查询并分析当前设备的实际状态，列出来源证据。';contextOpen=false"><WorkbenchIcon name="sparkles"/>向意构询问这台设备<WorkbenchIcon name="arrow-right"/></button>
              <details class="context-more"><summary>设备技术信息<WorkbenchIcon name="chevron-down"/></summary><dl class="device-facts"><div><dt>设备名称</dt><dd>{{selected.name}}</dd></div><div><dt>管理地址</dt><dd>{{selected.host}} : {{selected.port}}</dd></div><div><dt>驱动厂商</dt><dd>{{knownVendor(selected.vendor)}}</dd></div><div><dt>SSH 主机信任</dt><dd>{{selected.host_key_trusted?'已核验':'尚未核验'}}</dd></div><div><dt>LLDP 记录</dt><dd>{{selectedEvidence?.parsed?selectedEvidence.neighbors.length+' 条证据':'尚未采集'}}</dd></div></dl><label class="display-role">列表显示类型<select :value="roleFor(selected)" @change="selected&&saveRole(selected.id,($event.target as HTMLSelectElement).value as DeviceRole)"><option v-for="(label,id) in roleLabels" :key="id" :value="id">{{label}}</option></select><small>仅保存本机显示分类，不改变驱动或实际邻接。</small></label><div class="inspector-actions"><button :disabled="!!busy" @click="probeKey">核实 SSH 指纹</button><button :disabled="!selected.host_key_trusted||terminalOpening||!!busy" @click="openTerminal">打开 SSH 终端</button></div><div class="context-queries"><button v-for="[query,label] in [['version','设备信息'],['ports','端口状态'],['config','当前配置'],['neighbors','LLDP 邻居']]" :key="query" class="control" :disabled="!selected.connected||!!busy||!vendorHasQuery(selected.vendor,query)" @click="runQuery(query)">{{label}}</button></div></details>
              <details class="context-more"><summary>处理过程与记录 <small>{{latestStatus}}</small><WorkbenchIcon name="chevron-down"/></summary>              <div v-if="configView" class="execution-flow" :data-phase="configView.phase||'finished'">
                <div class="flow-title"><strong>最近一次配置</strong><span class="status-pill" :class="{succeeded:latestStatus==='succeeded',failed:latestStatus!=='succeeded'&&latestStatus!=='running'}">{{latestStatus}}</span></div>
                <ol><li v-for="(step,index) in configStages" :key="step.label" :class="'step-'+configStepState(index)"><span class="flow-stage-index">{{String(index+1).padStart(2,'0')}}</span><div><strong>{{step.label}}</strong><small>{{step.detail}}</small></div><em>{{configStepState(index)==='done'?'已完成':configStepState(index)==='active'?'进行中':configStepState(index)==='failed'?'失败':configStepState(index)==='unproven'?'未核验':'未开始'}}</em></li></ol>
                <p v-if="configView.error" class="flow-disclaimer error-text">{{configView.error}}</p>
                <button v-if="recentConfigTask?.result" class="flow-detail" @click="showTaskResult(recentConfigTask)">查看执行证据 ↗</button>
              </div>
              <div v-else class="inspector-quiet"><strong>无配置任务</strong><p>只有真实执行事件返回后才展示阶段状态。</p></div>
</details>
              <details class="context-more"><summary>功能说明<WorkbenchIcon name="chevron-down"/></summary>              <div class="capabilities"><strong>平台能力</strong><div><span>SSH 指纹核实 / 会话 / 人工终端</span><span>可用</span></div><div><span>只读查询（按厂商白名单）</span><span>{{selected?vendorTier[selected.vendor]||'未知':'—'}}</span></div><div><span>自动备份</span><span>{{selected&&canBackup(selected.vendor)?'可用':'该厂商未开放'}}</span></div><div><span>审批后配置写入</span><span>{{selected&&canWrite(selected.vendor)?'需人工批准及回读':'该厂商未开放'}}</span></div><div><span>多设备编排／自动回滚</span><span>待开发</span></div><p class="capability-note">各平台命令依据 CLI 手册候选；真实设备未验收（NOT_RUN），只读平台的型号兼容性需首次连接时验证。</p></div></details>
            </template><div v-else class="output-empty">尚未选择设备<span>请先添加并选择一台真实设备。</span><button class="control" :disabled="!desktop" @click="addOpen=true">添加设备</button></div>
          </aside>

        </div>
        <ToolDock :panel="panel" :height="dockHeight" :busy="busy" :expanded="terminalExpanded" @select="selectPanel" @resize="resizeDock" @step="stepDock">
          <template #actions><button class="control dock-expand" :aria-label="terminalExpanded?'退出终端全屏':'展开终端'" @click="terminalExpanded=!terminalExpanded;panel='terminal'"><WorkbenchIcon name="arrows-maximize"/><span>{{terminalExpanded?'还原':'终端'}}</span></button></template>
          <div v-if="panel==='tasks'" class="output execution-output"><div v-if="output" class="output-label">{{resultTitle}}<span class="source-label">来自真实 IPC 返回值</span></div><pre v-if="output">{{output}}</pre><div v-else class="output-empty">等待设备操作<span>查询输出、错误和备份结果将显示于此。</span></div></div>
          <div v-show="panel==='terminal'" class="terminal-container"><div class="terminal-tabs"><button class="terminal-add" :disabled="!selected?.host_key_trusted||terminalOpening||!!busy" @click="openTerminal"><WorkbenchIcon name="plus"/><span class="sr-only">新建会话</span></button><div v-for="t in terminals" :key="t.session" :class="{active:activeTerminal===t.session}"><button @click="activeTerminal=t.session">{{t.device.name}}</button><button :aria-label="'关闭 '+t.device.name+' 终端'" :disabled="closingTerminals.has(t.session)" @click="closeTerminal(t.session)"><WorkbenchIcon name="x"/></button></div></div><TerminalPane v-for="t in terminals" v-show="activeTerminal===t.session" :key="t.session" :session="t.session" :active="panel==='terminal'&&activeTerminal===t.session" :light="resolvedTheme==='light'" @activity="refreshTasks().catch(()=>{})"/><div v-if="!terminals.length" class="output-empty">没有终端会话<span>核实 SSH 主机指纹后可建立独立终端。</span><button class="control" :disabled="!selected?.host_key_trusted||terminalOpening||!!busy" @click="openTerminal">打开当前设备终端</button></div></div>
          <div v-if="panel==='history'" class="table-wrap"><div class="backup-toolbar"><button class="control" :disabled="!selected||!!busy" @click="reindexHistory">重建记录</button><button class="control" :disabled="!selected?.connected||!!busy||!canBackup(selected?.vendor||'')" @click="backup">备份当前配置</button></div><table v-if="histories.length"><thead><tr><th>备份时间</th><th>完整配置</th><th>Git 记录</th><th>操作</th></tr></thead><tbody><tr v-for="record in histories" :key="record.id"><td>{{new Date(record.created_at).toLocaleString()}}</td><td>{{record.bytes}} B · 加密</td><td><code>{{record.commit.slice(0,10)}}</code></td><td><button :disabled="!!busy" @click="verifyBackup(record)">校验与预览</button></td></tr></tbody></table><div v-else class="output-empty">暂无成功备份<span>设备连接时及运行期间按设备策略自动备份。</span></div></div>
          <div v-if="panel==='tasks'" class="table-wrap execution-tasks"><table v-if="selectedTasks.length"><thead><tr><th>时间</th><th>设备 / 操作</th><th>状态</th></tr></thead><tbody><tr v-for="task in selectedTasks" :key="task.id"><td>{{new Date(task.started_at).toLocaleTimeString()}}</td><td>{{devices.find(d=>d.id===task.device_id)?.name||'本地'}} · {{task.operation}}</td><td :class="{'status-error':task.status==='failed'}">{{task.status}}<small v-if="task.error">{{task.error}}</small><button v-if="task.result" @click="showTaskResult(task)">执行详情</button></td></tr></tbody></table><div v-else class="output-empty">暂无操作记录</div></div>
          <div v-if="panel==='details'" class="table-wrap device-metadata"><template v-if="selected"><p class="table-caption">登记设备元数据 · 状态来自本地核心</p><table><tbody><tr v-for="[label,value] in [['设备名称',selected.name],['管理地址',selected.host],['SSH 端口',selected.port],['驱动厂商',knownVendor(selected.vendor)],['查询连接',selected.connected?'已连接':'未连接'],['主机信任',selected.host_key_trusted?'已核验':'未核验']]" :key="label"><th>{{label}}</th><td>{{value}}</td></tr></tbody></table><button class="control" @click="contextOpen=true">打开设备面板</button></template><div v-else class="output-empty">尚未选择设备</div></div>
          <section v-if="panel==='diff'" class="diff-panel" aria-label="配置差异"><ConfigPlanDiff v-if="latestPlan" :plan="latestPlan" :device-name="selected?.name||'当前设备'" :busy="!!busy||!safetyApi||!selected?.connected" @review="latestPlan&&reviewPlan(latestPlan)"/><div v-else class="plan-empty"><strong>还没有可以对比的配置方案</strong><p>先描述变更目标，生成方案后查看中文说明与原始命令。</p><button class="control" :disabled="!selected?.connected||!safetyApi" @click="suggestTask('plan')">准备网络变更方案 <WorkbenchIcon name="arrow-right"/></button></div><details class="backup-comparison" :open="!latestPlan"><summary>比较两份加密备份</summary>
            <div class="diff-selectors"><label>基准备份<select v-model="diffBefore" @change="diffResult=null"><option value="">选择备份</option><option v-for="h in histories" :key="h.id" :value="h.id">{{new Date(h.created_at).toLocaleString()}} · {{h.id.slice(0,8)}}</option></select></label><label>对比备份<select v-model="diffAfter" @change="diffResult=null"><option value="">选择备份</option><option v-for="h in histories" :key="h.id" :value="h.id">{{new Date(h.created_at).toLocaleString()}} · {{h.id.slice(0,8)}}</option></select></label><button class="control" :disabled="!!busy||!diffBefore||!diffAfter||diffBefore===diffAfter" @click="compareBackups">校验并比较</button></div>
            <p class="diff-note">比较经过完整性校验的脱敏备份文本。相同脱敏内容不代表敏感字段相同；文本差异不代表设备已执行变更。</p>
            <template v-if="diffResult"><p class="diff-summary">{{diffResult.summary}}</p><details><summary>展开原始配置文本差异</summary><pre><span v-for="(line,index) in diffResult.lines" :key="index" :class="line.kind">{{line.kind==='added'?'+ ':line.kind==='removed'?'- ':'  '}}{{line.text}}
</span></pre></details></template>
            <div v-else class="output-empty">{{histories.length<2?'至少需要两份真实备份':'选择两份备份后进行比较'}}<span>没有备份时不会生成样例差异。</span></div>
          </details></section>
        </ToolDock>
      </main>
    </div>
    <footer class="status-bar"><span><WorkbenchIcon name="lock"/>本地设备凭据隔离</span><span>{{desktop?'Tauri 2 桌面客户端':'浏览器只读预览'}}</span><span class="spacer"></span><span>{{busy|| (aiBusy?'正在处理模型请求':'配置结果以设备回读为准')}}</span><span>YIGOU / 0.2.5</span></footer>
    <div v-if="helpOpen" class="modal-backdrop"><section class="modal help-dialog" role="dialog" aria-modal="true" aria-labelledby="help-title"><div class="modal-heading"><h2 id="help-title">开始使用意构</h2><button aria-label="关闭帮助" @click="helpOpen=false"><WorkbenchIcon name="x"/></button></div><ol><li>添加真实设备，凭据保存在系统凭据库。</li><li>打开设备详情，独立核实 SSH 主机指纹后连接。</li><li>使用“先检查，不修改”查询实际设备证据。</li><li>变更先生成方案，再逐项审批；自主处理须每轮明确授权。</li></ol><p>终端、执行记录、加密备份、配置差异和设备详情始终保留在底部。</p><label class="motion-settings">颜色主题<select :value="themeSetting" @change="setTheme(($event.target as HTMLSelectElement).value as 'dark'|'light'|'system')"><option value="light">浅色</option><option value="dark">深色</option><option value="system">跟随系统</option></select></label><button class="control" :disabled="!desktop||!!busy" @click="helpOpen=false;modelOpen=true">模型设置</button><label class="motion-settings">界面动效<select :value="motionSetting" @change="setMotion(($event.target as HTMLSelectElement).value as 'immersive'|'standard'|'static')"><option value="immersive">流畅</option><option value="standard">精简</option><option value="static">关闭</option></select></label><p class="form-note">{{runtime}} · {{safetyApi?'核心已声明审批接口能力':'AI 需配套新版安全核心'}}。动画仅响应真实请求和界面切换。</p></section></div>
    <div v-if="approvalOpen&&pendingPlan" class="modal-backdrop"><form class="modal approval-modal" role="dialog" aria-modal="true" aria-labelledby="approval-title" @submit.prevent="executeApproved">
      <div class="modal-heading"><h2 id="approval-title">设备配置 · 最终人工审批</h2><button type="button" aria-label="关闭配置审批" :disabled="!!busy" @click="closeApproval">×</button></div>
      <p class="approval-warning">此操作会真实修改设备。尚未备份或备份校验失败时，Rust 后端将拒绝下发；当前没有自动回滚功能。</p>
      <p>目标设备：{{devices.find(d=>d.id===pendingPlan?.device_id)?.name||pendingPlan.device_id}}</p>
      <p>计划编号：<code>{{pendingPlan.id}}</code></p>
      <p>保存启动配置：{{pendingPlan.config.save?'是':'否'}}</p>
      <div class="approval-commands"><strong>逐条核对将下发的命令</strong><ol><li v-for="(command,index) in pendingPlan.config.commands" :key="index"><code>{{command}}</code></li></ol></div>
      <div class="review-checks"><strong>执行后回读条件</strong><p v-for="(check,index) in pendingPlan.config.checks" :key="index">{{check.query}}：包含 {{check.contains.join('、')||'—'}}；不包含 {{check.not_contains.join('、')||'—'}}</p></div>
      <p class="approval-hash">审批绑定哈希：{{pendingPlan.request_hash}}</p>
      <label class="approval-check"><input v-model="approval.reviewed" type="checkbox" required>我已逐条审阅目标设备、具体命令、保存选项和回读条件</label>
      <label class="approval-check"><input v-model="approval.backup_ack" type="checkbox" required>我已了解系统会先生成并验证加密备份，失败将停止执行</label>
      <label class="approval-check"><input v-model="approval.rollback_ack" type="checkbox" required>我知悉设备执行可能产生部分变更、超时或未知结果，当前不会自动回滚或重试</label>
      <label>输入“确认执行”授权真实写入<input v-model="approval.phrase" required autocomplete="off" placeholder="确认执行"></label>
      <div class="modal-footer"><button type="button" :disabled="!!busy" @click="closeApproval">取消</button><button class="primary" type="submit" :disabled="!approvalReady||!!busy||!selected?.connected||selectedId!==pendingPlan.device_id">{{busy||'确认并执行一次'}}</button></div>
    </form></div>
    <div v-if="addOpen" class="modal-backdrop"><form class="modal" role="dialog" aria-modal="true" aria-labelledby="add-title" @submit.prevent="addDevice"><div class="modal-heading"><h2 id="add-title">添加设备</h2><button type="button" aria-label="关闭添加设备" :disabled="!!busy" @click="closeAdd">×</button></div><div class="form-grid"><label>设备名称<input v-model="newDevice.name" required maxlength="80" autofocus placeholder="例如 SW01"></label><label>厂商<select v-model="newDevice.vendor" @change="onVendorChange"><option value="huawei">华为</option><option value="ruijie">锐捷</option><option value="h3c">新华三</option><option value="cisco_ios">思科 IOS</option><option value="cisco_xe">思科 IOS-XE</option><option value="maipu">迈普</option><option value="raisecom_ros">瑞斯康达 ROS</option><option value="raisecom_roap">瑞斯康达 ROAP</option><option value="huawei_olt">华为 OLT</option><option value="huawei_smartax">华为 SmartAX</option><option value="zte_zxros">中兴 ZXROS</option></select><small class="vendor-tier-note">{{vendorTier[newDevice.vendor]||''}}</small></label><label>设备地址<input v-model="newDevice.host" required placeholder="IP 或主机名"></label><label>SSH 端口<input v-model.number="newDevice.port" type="number" required min="1" max="65535"></label><label>用户名<input v-model="newDevice.username" required autocomplete="off"></label><label>认证方式<select v-model="credential.kind"><option value="password">密码</option><option value="key">SSH 私钥文件</option></select></label><label v-if="credential.kind==='key'" class="full">私钥文件绝对路径<input v-model="credential.key_path" required placeholder="由本机用户管理的私钥文件"></label><label class="full">{{ credential.kind==='password'?'密码':'私钥口令（可选）' }}<input v-model="credential.secret" type="password" :required="credential.kind==='password'" autocomplete="new-password"></label><label class="check" :class="{disabled:!canBackup(newDevice.vendor)}"><input v-model="newDevice.auto_backup" type="checkbox" :disabled="!canBackup(newDevice.vendor)">自动备份配置（只读厂商不支持）</label><label>备份间隔（分钟）<input v-model.number="newDevice.backup_minutes" type="number" min="5" max="10080" required></label></div><p class="form-note">凭据保存在系统凭据库中。添加设备后需先核实 SSH 主机指纹。新厂商为只读阶段：SSH 连接、指纹核实与人工终端可用，部分查询按白名单开放，自动备份与自动配置写入未开放。</p><div class="modal-footer"><button type="button" :disabled="!!busy" @click="closeAdd">取消</button><button class="primary" :disabled="!!busy">{{ busy?'保存中…':'保存设备' }}</button></div></form></div>
    <div v-if="modelOpen" class="modal-backdrop"><form class="modal" role="dialog" aria-modal="true" aria-labelledby="model-title" @submit.prevent="saveModel"><div class="modal-heading"><h2 id="model-title">模型设置</h2><button type="button" aria-label="关闭模型设置" @click="model.api_key='';modelOpen=false">×</button></div><label>兼容接口地址<input v-model="model.base_url" type="url" required placeholder="https://模型服务地址/v1"></label><label>模型名称<input v-model="model.model" required placeholder="服务商提供的精确模型 ID"></label><label>API 密钥<input v-model="model.api_key" type="password" autocomplete="new-password" :placeholder="model.key_configured?'已保存；留空保留现有密钥':'本机免密服务可留空'"></label><p class="form-note">使用支持 Chat Completions、流式输出和工具调用的接口。远程地址需使用 HTTPS。设备密码不会发送给模型；查询输出先脱敏。</p><div class="modal-footer"><button class="primary" :disabled="!!busy">保存配置</button></div></form></div>
    <div v-if="trustOpen&&probe" class="modal-backdrop"><form class="modal" role="dialog" aria-modal="true" aria-labelledby="trust-title" @submit.prevent="trustKey"><div class="modal-heading"><h2 id="trust-title">核实 SSH 主机密钥</h2><button type="button" aria-label="关闭指纹核实" @click="trustOpen=false">×</button></div><p>设备：{{ trustDevice?.name }} · {{ trustDevice?.host }}</p><p>请通过设备控制台或可信管理员独立核对以下指纹。</p><code class="fingerprint">{{ probe.fingerprint }}</code><p class="form-note">读取指纹尚不代表信任设备。登记新密钥会关闭该设备的现有会话。</p><label>输入独立核实后的完整 SHA256 指纹<input v-model="confirmedFingerprint" required autocomplete="off" placeholder="SHA256:…"></label><div class="modal-footer"><button class="primary" :disabled="!!busy||confirmedFingerprint.trim()!==probe.fingerprint">已核实，登记密钥</button></div></form></div>
  </div>
</template>
