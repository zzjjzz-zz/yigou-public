# 意构 · Tauri 桌面前端集成记录

日期：2026-10-24。项目路径：`~/yigou`（当时的工作区路径，示例）。

## 本次集成

这是在原项目 `app/src` 上完成的 **Vue 3 + TypeScript + Tauri 2 原生客户端界面升级**，不是独立网页版。保留原 Rust/Tokio、Rig、Netmiko、SQLite、备份加密和 SSH PTY 逻辑，不添加网络服务或第二套业务后台。

| 区域 | 实现 | 实际来源 |
| --- | --- | --- |
| 双主题工作台 | 石墨黑 / 银白、系统跟随、动效三档、品牌和导航 | Vue/CSS，本地偏好 |
| 设备树与连接 | 按厂商分组、搜索、主机密钥核验、设备新增、连接/断开 | `device_list / device_add / host_key_probe / host_key_trust / device_connect / device_disconnect` |
| 自然语言工作区 | 对话、请求 ID、流式事件、工具证据、配置计划预览 | `ai_chat / ai_plan` 与 `yigou:event` |
| 网络拓扑 | SVG 拖动、缩放、源端与远端关联、邻居路径运动 | 只有 `device_discover` 返回 `parsed=true` 的 LLDP 记录才连线 |
| 交互终端 | 多标签、原始字节发送、Ctrl+C、跨包 UTF-8 输出、尺寸同步 | 原 `TerminalPane.vue` + `terminal_*` IPC；未克隆终端实例 |
| 执行结果 / 备份 / 任务 | 真实查询输出、加密备份校验、任务明细及错误 | `device_query / device_backup / backup_history / task_status` |
| 演示动效 | 示波器、主题变换、信号扫线、启动、导览 | 仅本地视觉动画，**不会调用真实设备或生成任务** |

`app/src/TopologyPane.vue` 不使用演示拓扑假数据：从原有 Python LLDP 解析器字段 `local_port`、`remote_name`、`remote_port`、`chassis_id` 和 `evidence` 渲染邻接关系。未发现邻居不等于无线，解析失败保持原始来源信息；只有名称匹配已登记设备时才能点选该设备，远端不会凭 LLDP 记录被标为已连接。

动态工作区在 `workspaceView` 之间切换；右侧上下文轨道采用设备真实元数据。原有模态表单及其 API 不变。静态模式和系统减少动画设置会停止场景动画。客户端浏览器预览仅做 UI 验证，不替代 Tauri 桌面后端，也不报告任何设备数据。

## 安全提醒

原后端的 `ai_chat` 目前具有直接配置设备能力，并**尚未强制逐命令人工审批**。界面已明确显示风险警告；勾选“生成配置计划预览”时调用 `ai_plan`，只允许查询/预览。**UI 提示不等于后端授权门禁，生产接入前仍需实现强制安全审核及真实型号验收。**

自动回滚、多设备编排、完整厂商虚拟机和跨机器备份密钥迁移仍未实现；新界面不声称这些功能已完成。

## 复现检查

- `pnpm --dir app build`：Vue TypeScript 类型检查与 Vite 生产构建通过。
- `YIGOU_CHROMIUM=/usr/bin/google-chrome CI=1 pnpm --dir app test`：**12/12 浏览器 E2E 检查通过**，包含既有 10 项和新增的 LLDP 证据、双主题及导览 2 项。原有多标签终端控制字符、部分字节确认、输入不重发的测试全部保留。
- 配置了不依赖 Playwright 默认下载的本机 Chromium 路径，避免误报“浏览器执行程序不存在”。
- 源码备份：`builds/frontend-baseline-0.2.0/App.vue`、`style.css`。
- Linux 发布脚本 `scripts/env.sh` 增加已安装 rustup 工具链路径识别，保持项目内 Cargo 构建缓存目录不变。
- Linux 原生安装包、完整系统安装卸载、Windows/macOS 原生交付需分别验证；**浏览器测试通过不等于三平台已经验收**。

相关接口契约：[docs/api.md](api.md)。原后端 0.2.0 验证记录：[docs/validation.md](validation.md)。
