import { invoke, isTauri } from '@tauri-apps/api/core'
export interface Device { id:string; name:string; host:string; port:number; vendor:string; username:string; connected:boolean; host_key_trusted:boolean; auto_backup:boolean; backup_minutes:number }
export interface Backup { id:string; device_id:string; created_at:string; bytes:number; sha256:string; commit:string }
export interface Task { id:string; device_id:string; operation:string; status:string; started_at:string; error?:string; result?:unknown }
export const desktop = isTauri()
export async function call<T>(command:string, args:Record<string,unknown> = {}):Promise<T> {
  if (!desktop) throw new Error('请通过意构桌面客户端使用设备功能。浏览器预览未连接 Rust 核心。')
  return invoke<T>(command,args)
}
export function errorText(error:unknown) { return error instanceof Error ? error.message : String(error) }

export interface ConfigPlan {id:string;device_id:string;request_hash:string;status:string;created_at:string;expires_at:number;config:{commands:string[];save:boolean;checks:{query:string;contains:string[];not_contains:string[]}[]}}
