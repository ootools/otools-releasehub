import { describe, expect, it } from 'vitest';
import { mount, unmount, flushSync } from 'svelte';
import Sidebar from '../components/Sidebar.svelte';
import Settings from '../pages/Settings.svelte';
import {createPreviewSnapshot} from './preview';
describe('plugin product surfaces',()=>{
  it('keeps library, downloads and GitHub without standalone AI navigation',async()=>{
    const snapshot=createPreviewSnapshot(),target=document.createElement('div');document.body.append(target);
    const component=mount(Sidebar,{target,props:{page:'home',snapshot,preview:false,onnavigate:()=>{},onadd:()=>{},ondata:()=>{}}});flushSync();
    expect(target.textContent).toContain('软件库');expect(target.textContent).toContain('GitHub');expect(target.textContent).toContain('下载');expect(target.textContent).not.toContain('AI 连接');expect(target.textContent).not.toContain('Windows x64');
    await unmount(component);target.remove();
  });
  it('retains update preferences without standalone tray settings or AI',async()=>{
    const snapshot=createPreviewSnapshot(),target=document.createElement('div');document.body.append(target);
    const component=mount(Settings,{target,props:{settings:snapshot.settings,runtime:snapshot.runtime,busy:false,preview:false,onsave:()=>{},onexport:()=>{},onimport:()=>{},ontextimport:()=>{},oncopyexport:()=>{},ondata:()=>{},ongithub:()=>{},onopen:()=>{}}});flushSync();
    expect(target.textContent).toContain('自动检查间隔');expect(target.textContent).toContain('同时下载数量');expect(target.textContent).not.toContain('托盘');expect(target.textContent).not.toContain('AI 连接');
    await unmount(component);target.remove();
  });
});
