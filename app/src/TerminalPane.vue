<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch, nextTick } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { call, errorText } from './api'
const props = defineProps<{session:string; active:boolean; light:boolean}>()
const emit = defineEmits<{activity:[]}>()
const element = ref<HTMLElement>()
let term:Terminal, fit:FitAddon, observer:ResizeObserver
let disposed=false, ended=false, reading=false, writing=false
let readTimer:ReturnType<typeof setTimeout>|undefined, writeTimer:ReturnType<typeof setTimeout>|undefined
let size:{cols:number;rows:number}|undefined
const inputs:{bytes:Uint8Array}[]=[]
const encoder=new TextEncoder()
function terminalTheme(light:boolean) {
  return light
    ? {background:'#f8faf9',foreground:'#28332e',cursor:'#315b48',selectionBackground:'#dce5df',black:'#28332e',white:'#f8faf9'}
    : {background:'#161d1b',foreground:'#edf3ee',cursor:'#a3d0b8',selectionBackground:'#3b5145',black:'#161d1b',white:'#edf3ee'}
}
function enqueue(bytes:Uint8Array, interrupt=false) {
  if(disposed||ended)return
  // Ctrl+C also cancels the unsent part of a paste; already accepted bytes are never replayed.
  if(interrupt)inputs.length=0
  for(let offset=0;offset<bytes.length;offset+=8192)inputs.push({bytes:bytes.slice(offset,offset+8192)})
  scheduleWrite(0)
}
function scheduleRead(delay=40) {
  if(disposed||ended||reading)return
  clearTimeout(readTimer);readTimer=setTimeout(()=>void readPump(),delay)
}
function scheduleWrite(delay:number) {
  if(disposed||ended||writing||!inputs.length)return
  clearTimeout(writeTimer);writeTimer=setTimeout(()=>void writePump(),delay)
}
function end(message:string) {
  if(ended)return
  ended=true;inputs.length=0;clearTimeout(readTimer);clearTimeout(writeTimer)
  if(!disposed){term.writeln(`\r\n[${message}]`);emit('activity')}
  void call('terminal_close',{session:props.session}).catch(()=>{})
}
async function readPump() {
  if(disposed||ended||reading)return
  reading=true
  try {
    if(size){const dimensions=size;size=undefined;await call('terminal_resize',{session:props.session,...dimensions})}
    const result=await call<{data:string;closed:boolean}>('terminal_read',{session:props.session})
    if(disposed)return
    if(result.data)await new Promise<void>(resolve=>term.write(Uint8Array.from(atob(result.data),c=>c.charCodeAt(0)),resolve))
    if(result.closed)end('SSH 会话已结束')
  } catch(error) {end(errorText(error))}
  finally {reading=false;scheduleRead()}
}
async function writePump() {
  if(disposed||ended||writing||!inputs.length)return
  writing=true;let delay=0
  try {
    for(let count=0;inputs.length&&count<8&&!disposed&&!ended;count++) {
      const frame=inputs[0], data=btoa(String.fromCharCode(...frame.bytes))
      const {sent_bytes:sent}=await call<{sent_bytes:number}>('terminal_write',{session:props.session,data})
      if(!Number.isInteger(sent)||sent<0||sent>frame.bytes.length)throw Error('TERMINAL_PROTOCOL: 发送确认无效')
      if(inputs[0]===frame){if(sent===frame.bytes.length)inputs.shift();else frame.bytes=frame.bytes.subarray(sent)}
      if(sent===0){delay=15;break}
    }
  } catch(error) {
    const message=errorText(error)
    if(message.includes('CONFIG_BUSY')){
      // The Rust gate rejected this frame. Never enqueue it for replay after
      // a guarded change, and do not close the independent human SSH session.
      inputs.length=0
      if(!disposed&&!ended){term.writeln('\r\n[设备配置操作占用连接；待发送输入已丢弃，终端保持连接]');emit('activity')}
    } else end(message)
  }
  finally {writing=false;scheduleWrite(delay)}
}
onMounted(()=>{
  term=new Terminal({cursorBlink:true,fontSize:12,lineHeight:1.6,fontFamily:'"JetBrains Mono Variable", "Cascadia Mono", Consolas, "Noto Sans SC Variable", monospace',scrollback:3000,screenReaderMode:true,theme:terminalTheme(props.light),allowProposedApi:false})
  fit=new FitAddon();term.loadAddon(fit);term.open(element.value!)
  term.onData(data=>enqueue(encoder.encode(data),data==='\x03'))
  term.onBinary(data=>enqueue(Uint8Array.from(data,c=>c.charCodeAt(0))))
  term.onResize(dimensions=>{size=dimensions;scheduleRead(0)})
  fit.fit();size={cols:term.cols,rows:term.rows}
  observer=new ResizeObserver(()=>{if(props.active)fit.fit()});observer.observe(element.value!)
  if(props.active)term.focus()
  scheduleRead(0)
})
watch(()=>props.active,async active=>{if(active){await nextTick();fit?.fit();term?.focus()}})
watch(()=>props.light,light=>{if(term)term.options.theme=terminalTheme(light)})
onBeforeUnmount(()=>{
  disposed=true;inputs.length=0;clearTimeout(readTimer);clearTimeout(writeTimer);observer?.disconnect();term?.dispose()
  void call('terminal_close',{session:props.session}).catch(()=>{})
})
</script>
<template><div ref="element" class="terminal-view" aria-label="交互式 SSH 终端" /></template>
