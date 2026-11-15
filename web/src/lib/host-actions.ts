import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { openPath, openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
export async function hostAction(action: string,payload: Record<string,unknown>):Promise<{handled:boolean;value?:unknown}> {
  const done=(value:unknown=null)=>({handled:true,value});
  switch(action){
    case 'pick_directory': {const selected=await open({directory:true,multiple:false,title:'选择软件目录'});return done(typeof selected==='string'?selected:null);}
    case 'export_library_file': {const path=await save({title:'导出追新订阅',defaultPath:'releasehub-library.json',filters:[{name:'JSON',extensions:['json']}]});return done(path?await invoke('dispatch',{action:'write_export_file',payload:{path}}):null);}
    case 'import_library_file': {const path=await open({title:'导入追新订阅',multiple:false,filters:[{name:'JSON',extensions:['json']}]});if(typeof path!=='string')return done();const contents=await invoke<string>('dispatch',{action:'read_text_file',payload:{path}});return done(await invoke('dispatch',{action:'import_library',payload:{contents}}));}
    case 'open_url': {const url=new URL(String(payload.url));if(url.protocol!=='https:'||url.username||url.password)throw new Error('只打开安全 HTTPS 链接。');await openUrl(url.toString());return done();}
    case 'open_download': {const path=await invoke<string>('dispatch',{action:'download_path',payload});await revealItemInDir(path);return done();}
    case 'open_data_directory': {const value=await invoke<{runtime:{dataDir:string}}>('dispatch',{action:'snapshot',payload:{}});await openPath(value.runtime.dataDir);return done();}
    case 'open_managed_version': {const path=await invoke<string>('dispatch',{action:'managed_path',payload});await openPath(path);return done();}
    default:return {handled:false};
  }
}
