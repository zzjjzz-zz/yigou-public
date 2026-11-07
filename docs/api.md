# 前端与业务接口 v1

Tauri invoke 参数使用 camelCase，业务数据字段使用 snake_case。错误通过失败 Promise 返回固定错误码与中文说明；失败时不能显示成功状态。原始设备异常和凭据不进入错误文本。接口可由后续任意前端实现调用。

| 接口 | 参数 | 成功结果 |
| --- | --- | --- |
| `device_add` | `device {name,host,port,vendor,username,auto_backup,backup_minutes}`, `credential {kind,secret,key_path}` | 设备元数据（不含凭据） |
| `device_list` | 无 | 本地设备及会话/主机信任状态 |
| `device_connect` | `deviceId` | 会话信息；开启自动备份时含独立备份状态 |
| `device_disconnect` | `deviceId` | 关闭该设备查询与终端会话 |
| `device_info` | `deviceId` | 版本命令及脱敏原文 |
| `device_query` | `deviceId`, `query: version/ports/config/neighbors/startup` | 命令和脱敏输出 |
| `device_discover` | `deviceId` | LLDP 命令、脱敏原文、结构化邻居、解析状态与来源文本 |
| `device_backup` | `deviceId` | 完整性验证后的加密备份记录 |
| `config_execute_approved` | `deviceId`, `planId`（UUID）, `reviewedHash`, `approval {reviewed,backup_ack,rollback_ack,phrase:"确认执行"}` | 仅执行与原计划哈希和设备会话一致、未过期且通过人工审批的命令；返回持久化执行记录，须检查 `status` |
| `backup_history` | `deviceId` | 记录 ID、时间、字节数、SHA256、Git commit |
| `backup_verify` | `deviceId`, `backupId` | 重新解密验证结果和脱敏预览 |
| `backup_reindex` | `deviceId` | 从通过解密校验且在 Git 中有记录的加密文件重建丢失的备份索引；设备记录必须存在 |
| `host_key_probe` | `deviceId` | 未认证 SSH 握手得到的密钥和 SHA256 指纹，不自动信任 |
| `host_key_trust` | `deviceId`, `algorithm`, `key`, `confirmedFingerprint` | 指纹匹配后在配置锁内断开旧会话、替换信任文件；与连接、自动配置互斥 |
| `terminal_open` | `deviceId` | 独立交互式 SSH PTY session；无需已连接查询会话 |
| `terminal_write` | `session`, `data`（原始字节的 Base64） | 正常时返回 `sent_bytes`；配置/连接锁占用时返回 `CONFIG_BUSY` 且不发送本次字节，客户端不可延迟重播此输入 |
| `terminal_read` | `session` | `data`（Base64 原始字节）、`closed`；无可读数据时立即返回空串 |
| `terminal_resize` | `session`, `cols`, `rows` | 同步远端 PTY 行列数 |
| `terminal_exec` | `session`, `command` | 人工命令便利接口；必要时追加 CR 后发送，返回 `sent_bytes`，实际结果从 `terminal_read` 读取 |
| `terminal_close` | `session` | 会话关闭结果 |
| `model_config_get` | 无 | 地址、模型名和密钥存在标记；不返回密钥 |
| `model_config_save` | `baseUrl`, `model`, `apiKey` | 非秘密配置；空密钥保留已保存密钥 |
| `ai_chat` / `ai_plan` | `deviceId`, `prompt`, `requestId`（UUID） | 只读对话 / 结构化计划生成；均返回 `executed:false`，后者可包含 `plan {id,device_id,request_hash,config,expires_at,status}`；无设备写入 |
| `ai_autonomous` | `deviceId`, `prompt`, `requestId`（UUID）, `consent:true` | 用户仅授权本次低风险自主执行；先查询并生成绑定本次请求的计划，再由 Rust 决定可否免审批执行。返回 `plan`、`configuration`、`executed` 和可能的 `autonomy_blocked`；危险操作拒绝后保留人工计划。 |
| `task_status` | `taskId`（可为空） | 指定或最近 100 项任务，含设备、操作、状态、起止时间与安全错误信息；配置任务含完整脱敏 `result` |
| `system_status` | 无 | 子进程 ping 的协议版本、客户端版本、数据目录、`ai_mode: optional_autonomous` 和 `terminal_mode: interactive` |

事件 `yigou:event`：`task`、`ai_delta`、`ai_tool_start`、`ai_tool_result`、`ai_config_start`、`ai_config_phase`、`ai_config_result`、`ai_plan_ready`、`ai_done`、`sessions_reset`、`terminal_closed`。普通 AI 事件按 request_id 关联；配置事件保留 plan_id 作为 request_id（持久任务编号），并携带可信 source_request_id（原 AI 请求编号，可为空）及 device_id；前端按来源关联对话、按设备归属缓存。完整备份原文和查询时的原始配置摘要不发事件、不进入模型上下文或 SQLite；`ai_delta` 只发送经过完整行敏感信息遮盖的文本，跨块未结束的行暂不发送。

`ai_chat` 只注册 `device_query`；`ai_plan` 和 `ai_autonomous` 在允许写入的厂商上额外注册只生成计划的 `device_propose_plan`，模型永远不获得真实配置写入工具。结构化计划包含 commands（1–256 条单行厂商命令，总计不超过 64 KiB）、save（是否保存启动配置）和 checks（至多 16 项 `{query,contains,not_contains}` 回读条件）；Rust 和 Python 均拒绝模式切换、退出、保存、重启、擦除及组合命令，也拒绝相应首词缩写；配置命令与核验条件若包含密码、口令、密钥等敏感材料，拒绝生成自动配置计划，需使用人工 SSH 终端。人工模式下用户须逐条审阅具体命令、保存选项、核验条件，确认变更前备份要求和没有自动回滚，再输入“确认执行”。自主模式仅对单设备单轮授权，要求成功查询当前运行配置，最多 12 条命令，只允许 VLAN 命名和接口描述等小范围变更；单独创建无可认证名称的 VLAN 只能走人工审批，回读条件必须覆盖每项实际修改；接口仅允许物理端口语法，拒绝 Loopback 等逻辑接口创建；风险不符时保持待审批计划，不允许模型自行升级权限。Rust 后端核对设备、会话、主机信任文件摘要、原始命令哈希及 15 分钟有效期（备份完成后再次检查），还要求本轮模型先查询完整运行配置；模型首次查询配置时，执行程序为同一次读取计算仅在 Rust 内部保存的原始摘要，并只把脱敏文本交给模型；生成计划时同时比较首次观察与计划基线的原始和脱敏摘要，敏感配置变化也会拒绝旧观察。计划基线来自同一次完整配置读取，避免两个不同时间的结果被错误绑定。审批前、加密备份后再次通过只读通道比对摘要，同时要求经解密验证的执行前备份内容摘要与获批配置基线一致；旧计划无摘要、配置或主机信任发生变化均拒绝执行。计划随后原子消耗，执行前完成加密备份及完整性验证，之后按计划原样执行并核验回读。`succeeded` 要求运行配置回读检查覆盖所有可识别的写入值，并确认它们位于目标接口或 VLAN 段落、全局配置的正确位置；任何要求的接口或 VLAN 都必须在回读中以完整标识出现，不能由其他修改成功或子串匹配替代；保存启动配置也要检查相同目标段落；接口标识比较忽略大小写，但描述、名称等写入值必须保持原始大小写一致。设备回执明确拒绝时不得计入已确认命令；未识别的修改语法只能标记未验证，不能依赖 sysname 或版本号等既有内容，且保存启动配置时必须验证持久化结果。

每轮最多生成一份结构化配置计划，默认需要人工审批；自主执行仅通过显式授权的 `ai_autonomous` 入口。该入口保存单次请求编号防重放，计划绑定原 AI 请求 ID，绝不直接开放无审批的配置执行接口。计划 ID 同时作为后续配置任务 ID：同一计划再次执行仅返回既有记录，不重放；更换计划哈希、设备或审批缺失均拒绝；未消费计划的会话或主机信任记录变化同样拒绝。已消费计划只返回历史执行记录，不把历史记录作为新会话执行结果。配置任务在下发前持久化；取消/断线后如已尝试写入则记 `unknown`，若尚未写入则 `not_started`。自动重试和自动回滚均不实施。状态包括 `succeeded`、`not_started`、`partial`、`save_failed`、`verification_failed`、`applied_unverified`、`applied_backup_failed`、`unknown`，并记录 attempted_commands/confirmed_commands、save_status、checks 和 before_backup/after_backup。confirmed_commands 只表示命令返回未报错，目标成功仍需回读。

工具固定绑定当前设备，不暴露人工终端、恢复或跨设备参数。人工终端是完整 SSH shell，设备账户权限范围内可执行任意命令；终端 session 与自动操作 session 在 Rust/Python 中分别管理且不可混用。为了防止同一客户端人工输入夹入自动配置窗口，配置锁占用期间终端输入立即拒绝，不存入延迟队列；自动配置审批也拒绝目标设备仍有打开的人工终端；关闭终端必须持有同一互斥锁直到底层 SSH 关闭完成，避免尚未结束的终端命令与批量下发交错。外部 SSH 会话无法被本地锁约束。终端输入输出只用于人工交互，不写入任务日志或 AI 上下文。

终端接口传递原始字节而非行命令：前端不得本地回显、过滤 ANSI、拼接提示符或等待命令完成后再允许输入。UTF-8 字符可跨帧，由 xterm.js 连续解码。`sent_bytes` 只表示本次实际被 SSH 接收的字节，客户端需持续读取输出，并仅重发尚未被确认的后缀；0 表示窗口暂满，不代表失败。terminal_exec 同样可能部分发送，应根据追加 CR 后的实际字节串处理余量。Ctrl+C 可取消待发送队列，不重放已确认字节。远端关闭时，最后的输出仍随 closed=true 返回。sessions_reset 只使自动操作会话失效，不移除人工终端；terminal_closed 带终端 ID、设备 ID 和可选错误。

后台资源限制：自动操作进程最多 16 个 SSH 会话；人工终端最多 16 个，每个使用独立进程。每次终端读取或写入最多 64 KiB。查询 RPC 超时 70 秒，配置 RPC 240 秒；单返回帧 12 MiB，查询输出上限 8 MiB。单轮 AI 最多 6 次模型调用、8 次查询和 1 份配置计划、480 秒总时限。自动操作 RPC 和 AI 对话各自串行；配置过程及连接更换串行，人工终端不受其队列占用。

实现参考：[Paramiko Channel API](https://docs.paramiko.org/en/stable/api/channel.html)。


本次 v6.2 集成仅作静态审阅，未编译、未测试。`ConfigPlanDiff.vue` 与 `api.ts` 使用的 `config.checks[].query/contains/not_contains` 均由 Rust 序列化提供；Tauri 命令参数为 camelCase，`approval` 内字段维持 snake_case。Python RPC 仍为协议 1 的 `configure {session,commands,save,checks}`，不接收 AI 自行声明的授权；审批由 Rust 执行入口校验，Python worker 不是公开 IPC 命令。

回读采用配置文本匹配，不能证明完整业务连通性、所有厂商语法或每个接口上下文的语义正确性；必须后续独立验收。模型生成的自然语言不是成功或授权证据，状态以持久化配置记录为准。自动执行返回错误时仍须查询记录，不可推断未写入。
