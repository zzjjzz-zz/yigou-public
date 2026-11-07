import { test, expect, type Page } from '../../app/node_modules/@playwright/test/index.js'

test('browser preview never pretends to have a backend or connected devices',async({page})=>{
  await page.goto('/')
  await expect(page.getByText('浏览器预览 · 设备功能不可用')).toBeVisible()
  await expect(page.getByRole('button',{name:'添加设备',exact:true})).toBeDisabled()
  await expect(page.getByRole('button',{name:'开始检查',exact:true})).toBeDisabled()
  await expect(page.getByText('尚未添加设备')).toBeVisible()
  await page.screenshot({path:'../builds/ui-preview.png',fullPage:true})
})

// Explicit IPC fixture for UI contract tests. Never included in application sources.
async function bridge(page:Page,connected=false,twoDevices=false){
  await page.addInitScript(({connected,twoDevices})=>{
    const w=window as unknown as Record<string,any>
    w.isTauri=true
    w.__terminalWrites=[];w.__terminalClosed=[];w.__terminalSizes=[]
    let nextSession=0
    const pending:Record<string,string>={}
    w.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},transformCallback:(callback:any)=>{w.__event=callback;return 1},unregisterCallback:()=>{},invoke:async(cmd:string,args:any)=>{
      if(cmd==='plugin:event|listen')return 1
      if(cmd==='plugin:event|unlisten')return
      if(cmd==='system_status')return {version:'fixture',ai_mode:'optional_autonomous'}
      if(cmd==='model_config_get')return null
      if(cmd==='task_status'||cmd==='backup_history')return []
      if(cmd==='device_list')return [{id:'fixture-device',name:'LAB-FIXTURE',host:'127.0.0.1',port:2222,vendor:'huawei',connected,host_key_trusted:true},...(twoDevices?[{id:'device-b',name:'LAB-B',host:'127.0.0.2',port:2222,vendor:'huawei',connected,host_key_trusted:true}]:[])]
      if(cmd==='terminal_open'){const session=`pty-${++nextSession}`;pending[session]='\r\n<LAB-FIXTURE>';return {session}}
      if(cmd==='terminal_read'){const data=pending[args.session]||'';pending[args.session]='';return {data:btoa(data),closed:false}}
      if(cmd==='terminal_write'){const bytes=atob(args.data);w.__terminalWrites.push({session:args.session,bytes});return {sent_bytes:bytes.length}}
      if(cmd==='terminal_resize'){w.__terminalSizes.push(args);return args}
      if(cmd==='terminal_close'){w.__terminalClosed.push(args.session);return {closed:true}}
      if(cmd==='device_query')return {command:'display interface brief',output:'FIXTURE ONLY: GE0/0/1 up up'}
      if(cmd==='device_add')throw 'KEYRING_UNAVAILABLE: 系统凭据库不可用或已锁定'
      if(cmd==='ai_chat')throw 'NOT_CONNECTED: 测试会话已断开'
      if(cmd==='model_config_save'){w.__lastSavedModel={baseUrl:args.baseUrl,model:args.model};return {base_url:args.baseUrl,model:args.model,key_configured:true}}
      throw `Unexpected fixture command: ${cmd}`
    }}
  },{connected,twoDevices})
}
test('credential failure stays visible and does not claim a device was saved',async({page})=>{
  await bridge(page);await page.goto('/');await page.getByRole('button',{name:'添加设备',exact:true}).click()
  await page.getByLabel('设备名称',{exact:true}).fill('SW01')
  await page.getByLabel('设备地址',{exact:true}).fill('127.0.0.1')
  await page.getByLabel('用户名',{exact:true}).fill('lab')
  await page.getByLabel('密码',{exact:true}).fill('fixture-only')
  await page.getByRole('button',{name:'保存设备',exact:true}).click()
  await expect(page.getByRole('alert')).toContainText('KEYRING_UNAVAILABLE')
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.getByRole('button',{name:'关闭添加设备'}).click()
  await page.getByRole('button',{name:'添加设备',exact:true}).click()
  await expect(page.getByLabel('密码',{exact:true})).toHaveValue('')
})
test('query result comes from IPC and model failures remain explicit',async({page})=>{
  await bridge(page,true);await page.goto('/')
  await page.getByRole('button',{name:'查看当前设备详情'}).click()
  await page.locator('.context-more').filter({hasText:'设备技术信息'}).locator('summary').click()
  await page.getByRole('button',{name:'端口状态',exact:true}).click()
  await expect(page.locator('.execution-output pre')).toContainText('FIXTURE ONLY: GE0/0/1 up up')
  await page.getByRole('button',{name:'关闭设备详情'}).click()
  await page.getByLabel('描述网络任务').fill('检查端口')
  await page.getByRole('button',{name:'开始检查',exact:true}).click()
  await expect(page.getByText('NOT_CONNECTED: 测试会话已断开')).toBeVisible()
})
test('model form stores exact model ID and clears the secret field',async({page})=>{
  await bridge(page);await page.goto('/');await page.getByRole('button',{name:'模型设置',exact:true}).click()
  await page.getByLabel('兼容接口地址').fill('https://example.invalid/v1')
  await page.getByLabel('模型名称').fill('vendor/exact-model-id')
  await page.getByLabel('API 密钥').fill('fixture-only')
  await page.getByRole('button',{name:'保存配置',exact:true}).click()
  await expect(page.getByRole('dialog')).not.toBeVisible()
  await page.getByRole('button',{name:'模型设置',exact:true}).click()
  await expect(page.getByLabel('API 密钥')).toHaveValue('')
  await expect(page.getByLabel('模型名称')).toHaveValue('vendor/exact-model-id')
})

test('manual SSH sends config, controls and multiline UTF-8 paste without query connection',async({page})=>{
  await bridge(page);await page.goto('/')
  await page.getByRole('button',{name:'新建会话',exact:true}).click()
  const input=page.locator('.xterm-helper-textarea')
  await input.focus()
  await page.keyboard.type('system-view');await page.keyboard.press('Enter')
  await page.keyboard.press('ArrowUp');await page.keyboard.press('Tab')
  await expect.poll(()=>page.evaluate(()=>(window as any).__terminalWrites.map((w:any)=>w.bytes).join(''))).toBe('system-view\r\x1b[A\t')
  await page.keyboard.press('Control+c')
  await input.evaluate(element=>{
    const clipboardData=new DataTransfer();clipboardData.setData('text/plain','interface GE0/0/1\ndescription 测试上联\nreturn\nsave\ny\n')
    element.dispatchEvent(new ClipboardEvent('paste',{clipboardData,bubbles:true,cancelable:true}))
  })
  await expect.poll(()=>page.evaluate(()=>{
    const bytes=(window as any).__terminalWrites.map((w:any)=>w.bytes).join('')
    return new TextDecoder().decode(Uint8Array.from(bytes as string,c=>c.charCodeAt(0)))
  })).toBe('system-view\r\x1b[A\t\x03interface GE0/0/1\rdescription 测试上联\rreturn\rsave\ry\r')
  await expect(page.getByText('受控只读终端')).toHaveCount(0)
  await expect.poll(()=>page.evaluate(()=>(window as any).__terminalSizes.length)).toBeGreaterThan(0)
  // Query worker resets leave the human PTY alive and receiving keyboard input.
  await page.evaluate(()=>(window as any).__event({payload:{kind:'sessions_reset'}}))
  await expect(input).toBeVisible()
  await input.focus();await page.keyboard.type('display version');await page.keyboard.press('Enter')
  await expect.poll(()=>page.evaluate(()=>(window as any).__terminalWrites.map((w:any)=>w.bytes).join(''))).toContain('display version\r')
  await page.screenshot({path:'../builds/ui-interactive-terminal.png',fullPage:true})
})

test('late device results stay with their originating device and terminals remain usable during a query',async({page})=>{
  await bridge(page,true,true);await page.goto('/')
  // Open B's independent PTY before the automatic query starts: a busy query
  // blocks *new* terminals, but must not block input on an existing terminal.
  await page.getByRole('button',{name:/LAB-B 华为/}).click()
  await page.getByRole('button',{name:'新建会话',exact:true}).click()
  await page.getByRole('button',{name:/LAB-FIXTURE 华为/}).click()
  await page.getByRole('button',{name:'查看当前设备详情'}).click()
  await page.locator('.context-more').filter({hasText:'设备技术信息'}).locator('summary').click()
  await page.evaluate(()=>{
    const w=window as any, original=w.__TAURI_INTERNALS__.invoke
    w.__TAURI_INTERNALS__.invoke=(cmd:string,args:any)=>cmd==='device_query'?new Promise(resolve=>{w.__resolveQuery=resolve}):original(cmd,args)
  })
  await page.getByRole('button',{name:'端口状态',exact:true}).click()
  await page.getByRole('button',{name:/LAB-B 华为/}).click()
  await page.getByRole('button',{name:'关闭设备详情'}).click()
  const input=page.locator('.terminal-view:visible .xterm-helper-textarea')
  await input.focus()
  await page.keyboard.type('display version')
  await page.keyboard.press('Enter')
  await expect.poll(()=>page.evaluate(()=>(window as any).__terminalWrites.filter((w:any)=>w.session==='pty-1').map((w:any)=>w.bytes).join(''))).toContain('display version\r')
  await page.getByRole('button',{name:'关闭 LAB-B 终端',exact:true}).click()
  await page.evaluate(()=>(window as any).__resolveQuery({command:'display interface brief',output:'EVIDENCE-FROM-A'}))
  await page.getByRole('button',{name:'执行记录',exact:true}).click()
  await expect(page.getByText('EVIDENCE-FROM-A')).toHaveCount(0)
  await page.getByRole('button',{name:/LAB-FIXTURE 华为/}).click()
  await expect(page.getByText('EVIDENCE-FROM-A')).toBeVisible()
})

test('late history and host key probe retain the originating device',async({page})=>{
  await bridge(page,true,true);await page.goto('/')
  await page.evaluate(()=>{
    const w=window as any, original=w.__TAURI_INTERNALS__.invoke
    w.__TAURI_INTERNALS__.invoke=(cmd:string,args:any)=>{
      if(cmd==='backup_history')return new Promise(resolve=>{(w.__resolveHistory??={})[args.deviceId]=resolve})
      if(cmd==='host_key_probe')return new Promise(resolve=>{(w.__resolveProbe??={})[args.deviceId]=resolve})
      if(cmd==='host_key_trust'){w.__trustedDevice=args.deviceId;return {trusted:true}}
      return original(cmd,args)
    }
  })
  await page.getByRole('button',{name:'加密备份',exact:true}).click()
  await page.getByRole('button',{name:/LAB-B 华为/}).click()
  await page.evaluate(()=>(window as any).__resolveHistory['fixture-device']([{id:'a',device_id:'fixture-device',created_at:'2026-01-01',bytes:91,commit:'only-a-commit'}]))
  await expect(page.getByText('only-a-com')).toHaveCount(0)
  await page.getByRole('button',{name:'查看当前设备详情'}).click()
  await page.locator('.context-more').filter({hasText:'设备技术信息'}).locator('summary').click()
  await page.getByRole('button',{name:'核实 SSH 指纹',exact:true}).click()
  await page.getByRole('button',{name:/LAB-FIXTURE 华为/}).click()
  await page.evaluate(()=>(window as any).__resolveProbe['device-b']({algorithm:'ssh-ed25519',key:'fixture',fingerprint:'SHA256:fixture-b'}))
  await expect(page.getByRole('dialog')).toContainText('LAB-B · 127.0.0.2')
  await page.getByLabel('输入独立核实后的完整 SHA256 指纹').fill('SHA256:fixture-b')
  await page.getByRole('button',{name:'已核实，登记密钥'}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__trustedDevice)).toBe('device-b')
})

test('terminal partial acknowledgements never duplicate input and Ctrl+C drops only unsent bytes',async({page})=>{
  await bridge(page);await page.goto('/')
  await page.evaluate(()=>{
    const w=window as any, original=w.__TAURI_INTERNALS__.invoke
    w.__accepted='';w.__blocked=false;w.__reads=0
    w.__TAURI_INTERNALS__.invoke=async(cmd:string,args:any)=>{
      if(cmd==='terminal_read')w.__reads++
      if(cmd==='terminal_write'){
        const bytes=atob(args.data)
        const count=w.__blocked&&bytes!=='\x03'?0:Math.min(2,bytes.length)
        w.__accepted+=bytes.slice(0,count)
        return {sent_bytes:count}
      }
      return original(cmd,args)
    }
  })
  await page.getByRole('button',{name:'新建会话',exact:true}).click()
  const input=page.locator('.xterm-helper-textarea')
  const paste=async(text:string)=>input.evaluate((element,text)=>{const clipboardData=new DataTransfer();clipboardData.setData('text/plain',text);element.dispatchEvent(new ClipboardEvent('paste',{clipboardData,bubbles:true,cancelable:true}))},text)
  await paste('abcdef\n')
  await expect.poll(()=>page.evaluate(()=>(window as any).__accepted)).toBe('abcdef\r')
  const before=await page.evaluate(()=>{const w=window as any;w.__blocked=true;return w.__reads})
  await paste('large-paste-must-be-cancelled')
  await expect.poll(()=>page.evaluate(()=>(window as any).__reads)).toBeGreaterThan(before+2)
  await input.focus();await page.keyboard.press('Control+c')
  await expect.poll(()=>page.evaluate(()=>(window as any).__accepted)).toBe('abcdef\r\x03')
  await page.evaluate(()=>(window as any).__blocked=false)
  await paste('next\n')
  await expect.poll(()=>page.evaluate(()=>(window as any).__accepted)).toBe('abcdef\r\x03next\r')
})

test('AI stays read-only until the exact plan receives explicit approval',async({page})=>{
  await bridge(page,true);await page.goto('/')
  await page.evaluate(()=>{
    const w=window as any, original=w.__TAURI_INTERNALS__.invoke
    w.__aiCalls=[];w.__executionArgs=null
    w.__TAURI_INTERNALS__.invoke=async(cmd:string,args:any)=>{
      if(cmd==='ai_chat'||cmd==='ai_plan'){
        w.__aiCalls.push(cmd)
        if(cmd==='ai_chat')return {text:'需要先生成计划并人工审批。',executed:false,plan:null}
        return {text:'已生成待审批计划，没有写入设备。',executed:false,plan:{id:'aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa',device_id:args.deviceId,request_hash:'reviewed-hash',status:'pending',created_at:'2026-11-01T00:00:00Z',expires_at:Math.floor(Date.now()/1000)+900,config:{commands:['interface GE0/0/1','description APPROVED'],save:true,checks:[{query:'config',contains:['description APPROVED'],not_contains:[]}]}}}
      }
      if(cmd==='config_execute_approved'){
        w.__executionArgs=args
        return {status:'verification_failed',write_attempted:true,error:'目标未匹配',before_backup:{id:'fixture-backup'}}
      }
      return original(cmd,args)
    }
  })
  await page.getByLabel('描述网络任务').fill('修改端口描述')
  await page.getByRole('button',{name:'开始检查',exact:true}).click()
  await expect(page.getByText('需要先生成计划并人工审批。')).toBeVisible()
  expect(await page.evaluate(()=>(window as any).__executionArgs)).toBeNull()
  await page.getByLabel('助手工作模式').selectOption('plan')
  await page.getByLabel('描述网络任务').fill('预览修改计划')
  await page.getByRole('button',{name:'生成方案',exact:true}).click()
  await expect(page.getByText(/配置计划预览 · 未执行/)).toBeVisible()
  await expect(page.locator('.reviewed-plan')).toContainText('description APPROVED')
  expect(await page.evaluate(()=>(window as any).__executionArgs)).toBeNull()
  await page.getByRole('button',{name:'审阅风险并审批'}).click()
  const approve=page.getByRole('dialog',{name:'设备配置 · 最终人工审批'})
  await expect(approve.getByRole('button',{name:'确认并执行一次'})).toBeDisabled()
  await approve.locator('.approval-check input').nth(0).check()
  await approve.locator('.approval-check input').nth(1).check()
  await approve.locator('.approval-check input').nth(2).check()
  await approve.getByPlaceholder('确认执行').fill('确认执行')
  await approve.getByRole('button',{name:'确认并执行一次'}).click()
  await expect(page.locator('.execution-output pre')).toContainText('verification_failed')
  const data=await page.evaluate(()=>(window as any).__executionArgs)
  expect(data).toMatchObject({deviceId:'fixture-device',planId:'aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa',reviewedHash:'reviewed-hash',approval:{reviewed:true,backup_ack:true,rollback_ack:true,phrase:'确认执行'}})
  expect(await page.evaluate(()=>(window as any).__aiCalls)).toEqual(['ai_chat','ai_plan'])
})

test('自主执行仅本轮授权，返回实际状态后不保留授权',async({page})=>{
  await bridge(page,true);await page.goto('/')
  await page.evaluate(()=>{
    const w=window as any,original=w.__TAURI_INTERNALS__.invoke
    w.__autoCalls=[]
    w.__TAURI_INTERNALS__.invoke=async(cmd:string,args:any)=>{
      if(cmd==='ai_autonomous'){
        w.__autoCalls.push(args)
        return {text:'自主执行记录：succeeded',preview_only:false,executed:true,
          plan:{id:'bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb',device_id:args.deviceId,request_hash:'checked',status:'consumed',config:{commands:['interface GE0/0/1','description AUTO'],save:false,checks:[{query:'config',contains:['description AUTO'],not_contains:[]}]},expires_at:Math.floor(Date.now()/1000)+900,created_at:'2026-11-01T00:00:00Z'},
          configuration:{status:'succeeded',write_attempted:true,phase:'finished',approval:{mode:'autonomous'}}}
      }
      return original(cmd,args)
    }
  })
  await page.getByLabel('助手工作模式').selectOption('autonomous')
  await expect(page.getByLabel('助手工作模式')).toHaveValue('autonomous')
  await page.getByLabel('描述网络任务').fill('更改接口描述')
  await expect(page.getByRole('button',{name:'授权并执行',exact:true})).toBeDisabled()
  await page.getByText('我授权 AI 对当前设备执行本次低风险配置',{exact:false}).click()
  await page.getByRole('button',{name:'授权并执行',exact:true}).click()
  await expect(page.getByText('自主执行记录：succeeded',{exact:false})).toBeVisible()
  await expect(page.locator('.execution-output pre')).toContainText('"mode": "autonomous"')
  expect(await page.evaluate(()=>(window as any).__autoCalls)).toEqual([expect.objectContaining({deviceId:'fixture-device',consent:true})])
  await expect(page.getByLabel('助手工作模式')).toHaveValue('analysis')
  await expect(page.getByRole('button',{name:'开始检查',exact:true})).toBeDisabled()
  await page.getByLabel('描述网络任务').fill('检查端口')
  await expect(page.getByRole('button',{name:'开始检查',exact:true})).toBeEnabled()
})

test('configuration events correlate by trusted source request, not the plan id',async({page})=>{
  await bridge(page,true);await page.goto('/')
  await page.evaluate(()=>{
    const w=window as any,original=w.__TAURI_INTERNALS__.invoke
    w.__TAURI_INTERNALS__.invoke=async(cmd:string,args:any)=>{
      if(cmd==='ai_autonomous'){
        w.__event({payload:{kind:'ai_config_result',request_id:'durable-plan-id',
          source_request_id:args.requestId,device_id:args.deviceId,
          result:{status:'verification_failed',write_attempted:true}}})
        return {text:'执行核验未通过',executed:false,plan:null,configuration:null}
      }
      return original(cmd,args)
    }
  })
  await page.getByLabel('助手工作模式').selectOption('autonomous')
  await page.getByLabel('描述网络任务').fill('检查来源关联')
  await page.getByText('我授权 AI 对当前设备执行本次低风险配置',{exact:false}).click()
  await page.getByRole('button',{name:'授权并执行',exact:true}).click()
  await expect(page.locator('.tool-events')).toContainText('配置执行：verification_failed')
})

test('terminal tabs route input independently and close only the selected session',async({page})=>{
  await bridge(page);await page.goto('/')
  const open=page.getByRole('button',{name:'新建会话',exact:true})
  await open.click();await open.click()
  await page.locator('.terminal-view:visible .xterm-helper-textarea').focus()
  await page.keyboard.type('configure terminal');await page.keyboard.press('Enter')
  await expect.poll(()=>page.evaluate(()=>(window as any).__terminalWrites.filter((w:any)=>w.session==='pty-2').map((w:any)=>w.bytes).join(''))).toBe('configure terminal\r')
  await page.getByRole('button',{name:'关闭 LAB-FIXTURE 终端',exact:true}).last().click()
  await expect(page.locator('.terminal-tabs > div')).toHaveCount(1)
  await page.locator('.terminal-view:visible .xterm-helper-textarea').focus()
  await page.keyboard.type('save');await page.keyboard.press('Enter')
  await expect.poll(()=>page.evaluate(()=>(window as any).__terminalWrites.filter((w:any)=>w.session==='pty-1').map((w:any)=>w.bytes).join(''))).toBe('save\r')
  expect(await page.evaluate(()=>(window as any).__terminalClosed)).not.toContain('pty-1')
  expect(await page.evaluate(()=>(window as any).__terminalClosed)).toContain('pty-2')
})

test('theme changes preserve the real SSH terminal session, input and bytes',async({page})=>{
  await bridge(page);await page.goto('/')
  await page.getByRole('button',{name:'新建会话',exact:true}).click()
  await expect(page.locator('.terminal-view:visible')).toBeVisible()
  await page.getByRole('button',{name:'浅色主题'}).click()
  await expect(page.locator('.app-shell')).toHaveAttribute('data-theme','light')
  const lightBackground=await page.locator('.terminal-container').evaluate(el=>getComputedStyle(el).backgroundColor)
  const input=page.locator('.terminal-view:visible .xterm-helper-textarea')
  await input.focus();await page.keyboard.type('display version');await page.keyboard.press('Enter')
  await expect.poll(()=>page.evaluate(()=>(window as any).__terminalWrites.map((w:any)=>w.bytes).join(''))).toBe('display version\r')
  await page.getByRole('button',{name:'深色主题'}).click()
  await expect(page.locator('.app-shell')).toHaveAttribute('data-theme','dark')
  const darkBackground=await page.locator('.terminal-container').evaluate(el=>getComputedStyle(el).backgroundColor)
  expect(darkBackground).not.toBe(lightBackground)
  expect(await page.evaluate(()=>(window as any).__terminalClosed)).toHaveLength(0)
  await expect(page.locator('.terminal-view')).toHaveCount(1)
})

test('topology shows only recorded LLDP edges and retains evidence when selecting another device',async({page})=>{
  await bridge(page,true,true);await page.goto('/')
  await page.getByRole('button',{name:'网络拓扑'}).first().click()
  await expect(page.locator('.topology-empty-note')).toContainText('尚无邻接证据')
  await expect(page.locator('.topology-link')).toHaveCount(0)
  await page.evaluate(()=>{
    const w=window as any,original=w.__TAURI_INTERNALS__.invoke
    w.__TAURI_INTERNALS__.invoke=(cmd:string,args:any)=>{
      if(cmd==='device_discover')return {command:'display lldp neighbor verbose',output:'LOCAL EVIDENCE ONLY',parsed:true,raw:'LOCAL EVIDENCE ONLY',neighbors:[{local_port:'GE0/0/1',remote_name:'LAB-B',remote_port:'GE0/0/24',protocol:'LLDP',evidence:'LOCAL EVIDENCE ONLY'}]}
      return original(cmd,args)
    }
  })
  await page.getByRole('button',{name:'发现当前设备'}).click()
  await expect(page.locator('.topology-link')).toHaveCount(1)
  await expect(page.locator('.topology-node.neighbor')).toHaveCount(2)
  await expect(page.locator('.topology-node.neighbor > text:not(.node-info)').filter({hasText:'LAB-B'})).toHaveCount(1)
  await expect(page.getByRole('button',{name:'重播链路追踪'})).toHaveCount(0)
  await page.getByRole('button',{name:/LAB-B 华为/}).click()
  await expect(page.locator('.topology-link')).toHaveCount(1)
  await expect(page.locator('.topology-node.selected')).toContainText('LAB-B')
})

test('desktop removes motion demonstration and fake telemetry; view transitions preserve safe state',async({page})=>{
  await bridge(page,true);await page.goto('/')
  await expect(page.getByText('播放完整动效')).toHaveCount(0)
  await expect(page.getByText('动效导览')).toHaveCount(0)
  await expect(page.locator('.signal-scope')).toHaveCount(0)
  await expect(page.locator('.machine-ports')).toHaveCount(0)
  await expect(page.locator('[data-tour]')).toHaveCount(0)
  await page.getByRole('button',{name:'浅色主题'}).click()
  await expect(page.locator('.app-shell')).toHaveAttribute('data-theme','light')
  const before=await page.evaluate(()=>(window as any).__terminalWrites.length)
  await page.getByRole('button',{name:'网络拓扑'}).first().click()
  await expect(page.locator('.topology-workspace')).toBeVisible()
  await expect(page.locator('.topology-link')).toHaveCount(0)
  await page.getByRole('button',{name:'展开终端'}).click()
  await expect(page.locator('.terminal-container')).toBeVisible()
  await page.getByRole('button',{name:'退出终端全屏'}).click()
  await page.getByRole('button',{name:'问题处理'}).click()
  await expect(page.locator('.assistant')).toBeVisible()
  expect(await page.evaluate(()=>(window as any).__terminalWrites.length)).toBe(before)
  await page.getByRole('button',{name:'深色主题'}).click()
  await expect(page.locator('.app-shell')).toHaveAttribute('data-theme','dark')
  await page.screenshot({path:'../builds/ui-desktop-refined.png',fullPage:true})
})

test('configuration timeline only follows real Rust task events',async({page})=>{
  await bridge(page,true);await page.goto('/')
  await expect(page.locator('.execution-flow')).toHaveCount(0)
  await page.getByRole('button',{name:'查看当前设备详情'}).click()
  await page.locator('.context-more').filter({hasText:'处理过程与记录'}).locator('summary').click()
  await expect(page.getByText('无配置任务')).toBeVisible()
  await page.evaluate(()=>{
    const event = new CustomEvent('yigou-e2e-config', {detail:{kind:'ai_config_start',device_id:'device-1'}})
    window.dispatchEvent(event)
  })
  await expect(page.locator('.execution-flow')).toHaveCount(0)
  // No hidden demo timer synthesizes config events or fabricated completed steps.
  expect(await page.evaluate(()=>(window as any).__terminalWrites.length)).toBe(0)
})

test('result pane can be resized without changing terminal process state',async({page})=>{
  await bridge(page,true);await page.goto('/')
  const panel=page.locator('.results')
  const before=await panel.boundingBox()
  await page.locator('.dock-resize').focus()
  await page.keyboard.press('ArrowUp')
  await expect.poll(async()=>Math.round((await panel.boundingBox())!.height)).toBeGreaterThan(Math.round(before!.height)+10)
  await page.keyboard.press('ArrowDown')
  await expect.poll(async()=>Math.abs((await panel.boundingBox())!.height-before!.height)).toBeLessThan(3)
})

// ---- Batch-1 read-only platforms: visible, grouped, and correctly gated ----
test('add dialog lists all batch-1 vendors with tier notes and disables auto-backup for read-only ones',async({page})=>{
  await bridge(page);await page.goto('/')
  await page.getByRole('button',{name:'添加设备',exact:true}).click()
  const vendors=[['huawei','华为','读写'],['cisco_ios','思科 IOS','只读'],['cisco_xe','思科 IOS-XE','只读'],['maipu','迈普','只读'],['raisecom_ros','瑞斯康达 ROS','只读'],['raisecom_roap','瑞斯康达 ROAP','只读'],['huawei_olt','华为 OLT','只读'],['huawei_smartax','华为 SmartAX','只读'],['zte_zxros','中兴 ZXROS','只读']]
  const dialog=page.getByRole('dialog')
  const vendorSelect=dialog.locator('select').first()
  for(const [value,label] of vendors){
    await vendorSelect.selectOption(value)
    await expect(vendorSelect).toHaveValue(value)
    await expect(dialog.locator('.vendor-tier-note')).toContainText(value==='huawei'?'读写':'只读')
  }
  await vendorSelect.selectOption('huawei')
  await expect(dialog.locator('.vendor-tier-note')).toContainText('读写')
  await vendorSelect.selectOption('zte_zxros')
  await expect(dialog.getByRole('checkbox')).toBeDisabled()
  await vendorSelect.selectOption('huawei')
  await expect(dialog.getByRole('checkbox')).toBeEnabled()
})

test('sidebar groups devices per vendor and quick actions follow the per-vendor whitelist',async({page})=>{
  await page.addInitScript(()=>{
    const w=window as unknown as Record<string,any>
    w.isTauri=true
    w.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(cmd:string)=>{
      if(cmd==='plugin:event|listen')return 1
      if(cmd==='plugin:event|unlisten')return
      if(cmd==='system_status')return {version:'fixture',ai_mode:'optional_autonomous'}
      if(cmd==='model_config_get')return null
      if(cmd==='task_status'||cmd==='backup_history')return []
      if(cmd==='device_list')return [
        {id:'dev-cisco',name:'CISCO-01',host:'127.0.0.3',port:22,vendor:'cisco_xe',connected:true,host_key_trusted:true},
        {id:'dev-zte',name:'ZTE-01',host:'127.0.0.4',port:22,vendor:'zte_zxros',connected:true,host_key_trusted:true},
        {id:'dev-olt',name:'OLT-01',host:'127.0.0.5',port:22,vendor:'huawei_olt',connected:true,host_key_trusted:true},
        {id:'dev-hw',name:'HW-01',host:'127.0.0.6',port:22,vendor:'huawei',connected:true,host_key_trusted:true},
      ]
      if(cmd==='device_query')return {command:'fixture',output:'FIXTURE OUTPUT'}
      throw `Unexpected fixture command: ${cmd}`
    }}
  })
  await page.goto('/')
  const sidebar=page.getByRole('navigation',{name:'设备列表'})
  // Current sidebar groups by displayed device role, not by manufacturer.
  for(const name of ['CISCO-01','ZTE-01','OLT-01','HW-01'])
    await expect(sidebar.getByRole('button',{name:new RegExp(name)})).toBeVisible()
  await page.getByRole('button',{name:'查看当前设备详情'}).click()
  await page.locator('.context-more').filter({hasText:'设备技术信息'}).locator('summary').click()
  await page.locator('.context-more').filter({hasText:'功能说明'}).locator('summary').click()
  const queries=page.locator('.context-queries')
  const checkQueries=async(allowed:string[])=>{
    for(const name of ['设备信息','端口状态','当前配置','LLDP 邻居']){
      const button=queries.getByRole('button',{name,exact:true})
      if(allowed.includes(name))await expect(button).toBeEnabled()
      else await expect(button).toBeDisabled()
    }
  }

  // Cisco IOS-XE: read-only queries, no automatic backups or writes.
  await sidebar.getByRole('button',{name:/CISCO-01/}).click()
  await checkQueries(['设备信息','端口状态','当前配置','LLDP 邻居'])
  await expect(page.locator('.capabilities')).toContainText('该厂商未开放')
  await page.getByRole('button',{name:'加密备份',exact:true}).click()
  await expect(page.getByRole('button',{name:'备份当前配置',exact:true})).toBeDisabled()

  // ZTE ZXROS: version only.
  await sidebar.getByRole('button',{name:/ZTE-01/}).click()
  await checkQueries(['设备信息'])
  await expect(page.getByRole('button',{name:'备份当前配置',exact:true})).toBeDisabled()

  // Huawei OLT: version and configuration queries.
  await sidebar.getByRole('button',{name:/OLT-01/}).click()
  await checkQueries(['设备信息','当前配置'])
  await expect(page.getByRole('button',{name:'备份当前配置',exact:true})).toBeDisabled()

  // Original Huawei platform retains permitted query and backup operations.
  await sidebar.getByRole('button',{name:/HW-01/}).click()
  await checkQueries(['设备信息','端口状态','当前配置','LLDP 邻居'])
  await expect(page.getByRole('button',{name:'备份当前配置',exact:true})).toBeEnabled()
})

test('browser preview loads project fonts and favicon without 4xx',async({page})=>{
  const fontStatuses:number[]=[],failures:string[]=[]
  page.on('response',response=>{
    const path=new URL(response.url()).pathname
    if(path.endsWith('.woff2'))fontStatuses.push(response.status())
    if(response.status()>=400)failures.push(`${response.status()} ${path}`)
  })
  await page.goto('/')
  await page.evaluate(()=>document.fonts.ready.then(()=>true))
  await expect.poll(()=>fontStatuses.length).toBeGreaterThan(0)
  expect(fontStatuses.every(status=>status===200)).toBe(true)
  expect(failures).toEqual([])
  expect(await page.evaluate(()=>document.fonts.check('16px "Noto Sans SC Variable"'))).toBe(true)
  await expect(page.locator('link[rel="icon"]')).toHaveAttribute('href','/favicon.ico')
  const favicon=await page.request.get('/favicon.ico')
  expect(favicon.status()).toBe(200)
  expect((await favicon.body()).subarray(0,4).toString('hex')).toBe('00000100')
})

test('add cancel, device search and settings open/close leave no side effects',async({page})=>{
  await bridge(page,true,true);await page.goto('/')
  await page.evaluate(()=>{
    const w=window as any,original=w.__TAURI_INTERNALS__.invoke
    w.__uiCalls=[]
    w.__TAURI_INTERNALS__.invoke=(cmd:string,args:any)=>{w.__uiCalls.push(cmd);return original(cmd,args)}
  })
  // Cancel the add-device form without saving.
  await page.getByRole('button',{name:'添加设备',exact:true}).click()
  await page.getByLabel('设备名称',{exact:true}).fill('TEMP-DEVICE')
  await page.getByRole('button',{name:'取消',exact:true}).click()
  await expect(page.getByRole('dialog')).not.toBeVisible()
  // Reopening and closing the form keeps the draft but never creates a device.
  await page.getByRole('button',{name:'添加设备',exact:true}).click()
  await page.getByRole('button',{name:'关闭添加设备'}).click()
  await expect(page.getByRole('dialog')).not.toBeVisible()
  // Search filters the existing list and clears back to all devices.
  const search=page.getByLabel('搜索设备',{exact:true})
  await page.locator('.command-search').click()
  await expect(search).toBeFocused()
  await search.fill('LAB-B')
  await expect(page.locator('.device-list .device')).toHaveCount(1)
  await expect(page.getByRole('button',{name:/LAB-FIXTURE 华为/})).toHaveCount(0)
  await search.fill('no-such-device')
  await expect(page.getByText('未找到匹配的设备')).toBeVisible()
  await search.fill('')
  await expect(page.locator('.device-list .device')).toHaveCount(2)
  // Settings open and close without saving model configuration.
  await page.getByRole('button',{name:'模型设置',exact:true}).click()
  await expect(page.getByRole('dialog',{name:'模型设置'})).toBeVisible()
  await page.getByRole('button',{name:'关闭模型设置'}).click()
  await expect(page.getByRole('dialog')).not.toBeVisible()
  const calls=await page.evaluate(()=>(window as any).__uiCalls)
  expect(calls.filter((cmd:string)=>['device_add','model_config_save','terminal_open','config_execute_approved','ai_autonomous'].includes(cmd))).toEqual([])
})
