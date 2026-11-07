// Display preferences never change a device driver, capabilities or observed links.
export type DeviceRole='core'|'access'|'switch'|'router'|'firewall'|'unknown'
export const roleLabels:Record<DeviceRole,string>={core:'核心交换机',access:'接入交换机',switch:'交换机',router:'路由器',firewall:'防火墙',unknown:'未分类设备'}
export function namedRole(name:string):DeviceRole {
  if(/核心|(^|[-_ ])core([-_ ]|$)/i.test(name))return 'core'
  if(/接入|(^|[-_ ])(access|acc)([-_ ]|$)/i.test(name))return 'access'
  if(/防火墙|(^|[-_ ])(firewall|fw)([-_ ]|$)/i.test(name))return 'firewall'
  if(/路由|(^|[-_ ])(router|rtr)([-_ ]|$)/i.test(name))return 'router'
  if(/交换机|(^|[-_ ])(switch|sw)([-_ ]|$)/i.test(name))return 'switch'
  return 'unknown'
}
export function readRoles():Record<string,DeviceRole>{
  try{const value:unknown=JSON.parse(localStorage.getItem('yigou:device-roles')||'{}');if(!value||typeof value!=='object'||Array.isArray(value))return {};return Object.fromEntries(Object.entries(value).filter(([,role])=>typeof role==='string'&&Object.prototype.hasOwnProperty.call(roleLabels,role))) as Record<string,DeviceRole>}catch{return {}}
}
