<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { Device } from './api'
import DeviceGlyph from './DeviceGlyph.vue'
import WorkbenchIcon from './WorkbenchIcon.vue'
import { namedRole, type DeviceRole } from './device-presentation'
export interface LldpNeighbor {local_port?:string|null;remote_name?:string|null;remote_port?:string|null;chassis_id?:string|null;protocol?:string;evidence?:string}
export interface DiscoveryRecord {neighbors:LldpNeighbor[];parsed:boolean;raw?:string;note?:string|null;checkedAt?:string}
const props=defineProps<{devices:Device[];selectedId:string;records:Record<string,DiscoveryRecord>;loading:boolean;motion:'immersive'|'standard'|'static';discoverySupported:boolean;roles:Record<string,DeviceRole>}>()
const emit=defineEmits<{select:[id:string];discover:[]}>()
const selected=computed(()=>props.devices.find(d=>d.id===props.selectedId))
const zoom=ref(1),hover=ref(''),observedSelection=ref(''),pan=ref({x:0,y:0})
let drag:{x:number;y:number;px:number;py:number}|undefined
const role=(id:string,name:string)=>props.roles[id]||namedRole(name)
const tier=(kind:DeviceRole)=>kind==='core'?1:kind==='access'?2:kind==='router'||kind==='firewall'?0:3
const tierNames=['边界设备','核心交换机','接入交换机','未标注层级']
const edges=computed(()=>Object.entries(props.records).flatMap(([source,record])=>{
  if(!record.parsed||!props.devices.some(d=>d.id===source))return []
  return record.neighbors.map((n,index)=>{
    const label=n.remote_name?.trim()||n.chassis_id?.trim()||'未命名邻居'
    const matches=props.devices.filter(d=>d.name.toLowerCase()===label.toLowerCase())
    const known=matches.length===1?matches[0]:undefined
    const identity=n.chassis_id?.trim()||n.remote_name?.trim()||'anonymous:'+index
    return {key:source+':'+index,source,target:known?.id||source+':peer:'+identity,label,known,local:n.local_port||'本地端口未报告',remote:n.remote_port||'远端端口未报告',at:record.checkedAt}
  }).filter(e=>e.source!==e.target)
}))
const nodes=computed(()=>{
  const all:{id:string;name:string;known?:Device;role:DeviceRole;tier:number}[]=props.devices.map(d=>({id:d.id,name:d.name,known:d,role:role(d.id,d.name),tier:tier(role(d.id,d.name))}))
  for(const edge of edges.value)if(!all.some(d=>d.id===edge.target))all.push({id:edge.target,name:edge.label,known:undefined,role:namedRole(edge.label),tier:tier(namedRole(edge.label))})
  let y=58
  return tierNames.flatMap((_,level)=>{
    const group=all.filter(d=>d.tier===level),columns=level===0?1:3
    const result=group.map((d,index)=>{const count=Math.min(columns,group.length-Math.floor(index/columns)*columns);return {...d,x:490+(index%columns-(count-1)/2)*310,y:y+Math.floor(index/columns)*95}})
    if(group.length)y+=Math.ceil(group.length/columns)*95+42
    return result
  })
})
const chartHeight=computed(()=>Math.max(500,...nodes.value.map(d=>d.y+65)))
const layers=computed(()=>tierNames.flatMap((label,level)=>{const group=nodes.value.filter(n=>n.tier===level);return group.length?[{label,y:group[0].y-46,height:group[group.length-1].y-group[0].y+91}]:[]}))
const graphSelection=computed(()=>nodes.value.some(n=>n.id===observedSelection.value)?observedSelection.value:props.selectedId)
const active=computed(()=>hover.value||graphSelection.value)
const activeNode=computed(()=>nodes.value.find(n=>n.id===active.value))
watch(()=>props.selectedId,()=>{observedSelection.value='';hover.value=''})
function selectNode(node:{id:string;known?:Device}){hover.value='';if(node.known){observedSelection.value='';emit('select',node.id)}else observedSelection.value=node.id}
const adjacent=computed(()=>new Set(edges.value.filter(e=>e.source===active.value||e.target===active.value).flatMap(e=>[e.source,e.target])))
function neighbors(id:string){return [...new Set(edges.value.filter(e=>e.source===id||e.target===id).map(e=>nodes.value.find(n=>n.id===(e.source===id?e.target:e.source))?.name||''))].filter(Boolean).join('、')||'尚无邻接证据'}
function position(id:string){return nodes.value.find(n=>n.id===id)}
function route(source:string,target:string){const a=position(source),b=position(target);if(!a||!b)return '';if(a.y===b.y){const direction=b.x>a.x?1:-1;return `M${a.x+70*direction} ${a.y} L${b.x-70*direction} ${b.y}`}const direction=b.y>a.y?1:-1,mid=(a.y+b.y)/2;return `M${a.x} ${a.y+33*direction} C${a.x} ${mid} ${b.x} ${mid} ${b.x} ${b.y-33*direction}`}
function down(e:PointerEvent){if(e.button!==0||(e.target as Element).closest('[data-node]'))return;drag={x:e.clientX,y:e.clientY,px:pan.value.x,py:pan.value.y};(e.currentTarget as SVGElement).setPointerCapture(e.pointerId)}
function move(e:PointerEvent){if(!drag)return;const r=(e.currentTarget as SVGElement).getBoundingClientRect();if(r.width&&r.height)pan.value={x:drag.px+(e.clientX-drag.x)*980/r.width,y:drag.py+(e.clientY-drag.y)*chartHeight.value/r.height}}
function reset(){pan.value={x:0,y:0};zoom.value=1}
function topologyKey(event:KeyboardEvent){if(!['+','-','0'].includes(event.key))return;event.preventDefault();if(event.key==='0')reset();else zoom.value=Math.max(.55,Math.min(1.8,zoom.value+(event.key==='+' ? .1 : -.1)))}
</script>
<template>
  <section class="topology-workspace" aria-label="网络拓扑视图" @keydown="topologyKey">
    <header class="topology-heading"><div><h2>网络拓扑</h2><p>一眼看清设备如何连接。点击任意设备，查看它连向哪里。</p><small>{{devices.length}} 台登记设备 · {{edges.length}} 条邻居记录</small></div><button class="control" :disabled="!selected?.connected||!discoverySupported||loading" @click="emit('discover')"><WorkbenchIcon name="topology-star-3"/>{{loading?'采集中…':'发现当前设备'}}</button></header>
    <p class="topology-note">连线来自真实 LLDP 记录，层级采用本机显示分类或设备名称；未标注的设备单独列出。</p>
    <div class="topology-chart">
      <svg class="topology-svg" :viewBox="'0 0 980 '+chartHeight" role="group" aria-label="LLDP 证据拓扑，可拖动、缩放并选择设备" @pointerdown="down" @pointermove="move" @pointerup="drag=undefined" @pointercancel="drag=undefined" @wheel.prevent="zoom=Math.max(.55,Math.min(1.8,zoom+($event.deltaY>0?-.08:.08)))">
        <g :transform="`translate(${pan.x} ${pan.y}) scale(${zoom})`">
          <g class="topology-layers" aria-hidden="true"><g v-for="layer in layers" :key="layer.label"><rect x="60" :y="layer.y" width="860" :height="layer.height" rx="18"/><text x="80" :y="layer.y+20" class="tier-label">{{layer.label}}</text></g></g>
          <g v-for="edge in edges" :key="edge.key" :class="{highlight:edge.source===active||edge.target===active}" class="topology-link"><title>{{edge.local}} → {{edge.remote}} · {{edge.at||'采集时间未知'}}</title><path :d="route(edge.source,edge.target)"/></g>
          <g v-for="node in nodes" :key="node.id" data-node class="topology-node" :class="{selected:node.id===graphSelection,neighbor:adjacent.has(node.id),observed:!node.known}" :transform="`translate(${node.x} ${node.y})`" role="button" :tabindex="0" :aria-pressed="node.id===graphSelection" :aria-label="node.name+'，'+neighbors(node.id)" @click="selectNode(node)" @keydown.enter="selectNode(node)" @keydown.space.prevent="selectNode(node)" @pointerenter="hover=node.id" @pointerleave="hover=''" @focus="hover=node.id" @blur="hover=''">
            <rect x="-70" y="-33" width="140" height="66" rx="8"/><foreignObject x="-58" y="-18" width="24" height="19"><DeviceGlyph class="topology-glyph" :name="node.name" :role="node.role"/></foreignObject><text x="-29" y="-5">{{node.name.length>12?node.name.slice(0,11)+'…':node.name}}</text><text class="node-info" x="-55" y="19">{{node.known?(node.known.connected?'查询已连接':'未连接'):'仅观察记录'}}</text><title>{{node.name}} · {{neighbors(node.id)}}</title>
          </g>
        </g>
      </svg>
      <div v-if="!devices.length" class="output-empty">暂无真实设备<span>添加设备并采集 LLDP 后展示连接。</span></div>
      <div v-else-if="!edges.length" class="topology-empty-note">{{records[selectedId]?.note||'尚无邻接证据。连接设备后点击“发现当前设备”。'}} 未发现邻居不代表没有链路。</div>
      <div class="topology-controls"><button aria-label="缩小" @click="zoom=Math.max(.55,zoom-.1)"><WorkbenchIcon name="minus"/></button><span>{{Math.round(zoom*100)}}%</span><button aria-label="放大" @click="zoom=Math.min(1.8,zoom+.1)"><WorkbenchIcon name="plus"/></button><button aria-label="重置视图" @click="reset"><WorkbenchIcon name="focus-2"/></button></div>
    </div>
    <section class="mobile-topology" aria-label="分层设备关系"><div v-for="(label,level) in tierNames" :key="label" class="mobile-topology-tier"><template v-if="nodes.some(n=>n.tier===level)"><div class="mobile-tier-heading">{{label}}<small>{{nodes.filter(n=>n.tier===level).length}} 台</small></div><div class="mobile-tier-nodes"><button v-for="node in nodes.filter(n=>n.tier===level)" :key="node.id" class="mobile-topology-node" :class="{selected:node.id===graphSelection,'is-neighbor':adjacent.has(node.id)}" :aria-pressed="node.id===graphSelection" @click="selectNode(node)"><DeviceGlyph class="mobile-glyph" :name="node.name" :role="node.role"/><span class="mobile-topology-copy"><strong>{{node.name}}</strong><small>直连：{{neighbors(node.id)}}</small><small v-if="!node.known">仅观察记录 · 未登记设备</small></span><i class="mobile-node-state"/></button></div></template></div><p v-if="!edges.length" class="topology-note">尚无采集到的连接证据。</p></section>
    <div v-if="activeNode" class="topology-tooltip"><strong>{{activeNode.name}}</strong><small>直连：{{neighbors(activeNode.id)}} · {{activeNode.known?(activeNode.known.connected?'查询已连接':'未连接'):'仅观察记录'}}</small></div>
    <footer class="topology-bottom-row"><span>高亮线路与选中设备直接相连</span><span>LLDP 观察记录 · 在线状态来自设备 API</span></footer>
  </section>
</template>
<style scoped>
.topology-workspace{position:relative;display:flex;flex:1;min-width:0;min-height:0;flex-direction:column;gap:0;overflow:hidden;background:var(--bg)}
.topology-heading{display:flex;justify-content:space-between;gap:12px;align-items:start;padding:18px 22px 0;background:var(--surface)}.topology-heading h2{font-size:24px;font-weight:650;line-height:1.3}.topology-heading p,.topology-heading small,.topology-note{font-size:12px;color:var(--muted);line-height:1.6}.topology-heading p{margin:6px 0}.topology-heading .control{display:flex;align-items:center;gap:7px;white-space:nowrap;min-height:34px}
.topology-note{padding:8px 22px 12px;background:var(--surface);border-bottom:1px solid var(--line)}.topology-chart{position:relative;flex:1;min-height:0;background-image:radial-gradient(var(--grid) 1px,transparent 1px);background-size:19px 19px;overflow:hidden}.topology-svg{width:100%;height:100%;touch-action:none;cursor:grab}.topology-svg:active{cursor:grabbing}.topology-layers rect{fill:none;stroke:var(--line);stroke-width:1}.tier-label{font-size:12px;fill:var(--muted)}.topology-link path{fill:none;stroke:color-mix(in srgb,var(--muted) 42%,var(--line));stroke-width:1.7}.topology-link.highlight path{stroke:var(--accent);stroke-width:1.8}.topology-node{cursor:pointer;outline:none}.topology-node>rect{fill:var(--surface);stroke:var(--border);transition:stroke .18s,fill .18s}.topology-node.selected>rect{stroke:var(--accent);stroke-width:1.7;fill:var(--accent-soft)}.topology-node.neighbor>rect,.topology-node:focus-visible>rect{stroke:var(--accent)}.topology-node.observed>rect{stroke-dasharray:4 3}.topology-node text{fill:var(--fg);font:13px var(--font-body)}.topology-node .node-info{font:11px var(--font-mono);fill:var(--muted)}.topology-controls{position:absolute;right:20px;bottom:12px;display:flex;align-items:center;gap:3px;background:var(--surface);border:1px solid var(--border);border-radius:6px;padding:3px}.topology-controls button{display:grid;place-items:center;width:30px;height:30px;border:0;color:var(--muted)}.topology-controls .icon{width:16px;height:16px}.topology-controls span{font-size:12px;min-width:39px;text-align:center}.topology-empty-note{position:absolute;bottom:58px;left:20px;right:20px;font-size:12px;color:var(--muted);padding:8px;background:var(--surface)}.topology-tooltip{flex:0 0 auto;align-self:start;margin:0 20px 8px;padding:9px 12px;border:1px solid var(--border);border-radius:6px;background:var(--surface);font-size:12px;max-width:70%}.topology-tooltip strong{font-weight:500}.topology-tooltip small{display:block;font-size:12px;color:var(--muted);margin-top:5px;overflow-wrap:anywhere}.topology-bottom-row{display:flex;justify-content:space-between;gap:8px;padding:0 20px 17px;font-size:12px;color:var(--muted)}.mobile-topology{display:none}
@media(max-height:780px) and (min-width:761px){.topology-heading{padding:8px 15px}.topology-heading h2{font-size:21px}.topology-note{padding:0 15px 8px}.topology-tooltip{position:absolute;top:95px;right:12px;max-width:45%;margin:0;font-size:12px}.topology-bottom-row{display:none}}
@media(max-width:760px){.topology-heading{padding:10px 14px 7px;gap:6px;align-items:center}.topology-heading h2{font-size:19px}.topology-heading p{font-size:12px;line-height:1.4;margin-top:4px}.topology-heading small{display:none}.topology-heading .control{padding:6px 9px;font-size:12px;min-height:36px}.topology-heading .icon{display:none}.topology-note{font-size:12px;padding:6px 12px 10px;border:0;background:var(--bg)}.topology-chart{display:none}.mobile-topology{display:block;flex:1;min-height:0;overflow:auto;padding:6px 12px 16px}.mobile-topology-tier{position:relative;margin:0 0 10px}.mobile-tier-heading{display:flex;justify-content:space-between;align-items:center;gap:8px;font-size:12px;font-weight:630;margin:0 0 5px}.mobile-tier-heading small{font-weight:400;color:var(--muted);font-size:12px}.mobile-tier-nodes{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:7px}.mobile-topology-node{display:flex;align-items:start;gap:7px;padding:8px 7px;min-height:71px;border:1px solid var(--border);border-radius:8px;text-align:left;min-width:0;background:var(--surface)!important}.mobile-topology-copy{display:flex;flex-direction:column;gap:5px;min-width:0;flex:1}.mobile-topology-node strong{font-size:12px;font-weight:650;line-height:1.4;overflow-wrap:anywhere}.mobile-topology-node small{font-size:11px;color:var(--muted);line-height:1.45;overflow-wrap:anywhere}.mobile-node-state{flex:0 0 5px;width:5px;height:5px;margin-top:4px;border:1px solid var(--muted);border-radius:50%}.mobile-topology-node.selected{background:var(--accent-soft)!important;border-color:var(--accent);box-shadow:inset 3px 0 var(--accent)}.mobile-topology-node.selected>.mobile-node-state{background:var(--accent);border-color:var(--accent)}.mobile-topology-node.is-neighbor{border-color:var(--accent)}.mobile-topology-node.is-neighbor>.mobile-node-state{background:var(--muted)}.topology-bottom-row,.topology-tooltip{display:none}}
@media(prefers-reduced-motion:reduce){*{transition:none!important}}
</style>
