import {beforeEach,describe,expect,it,vi}from'vitest';
import{mount,unmount,flushSync}from'svelte';
import App from '../App.svelte';import{createPreviewSnapshot}from'./preview';
const api=vi.hoisted(()=>({dispatch:vi.fn(),watchChanges:vi.fn()}));
vi.mock('./bridge',()=>({desktopAvailable:()=>true,dispatch:api.dispatch,watchChanges:api.watchChanges}));
const settle=async()=>{await new Promise(resolve=>setTimeout(resolve,0));flushSync();};
beforeEach(()=>{api.dispatch.mockReset();api.watchChanges.mockResolvedValue(()=>{});window.matchMedia=()=>({matches:false,addEventListener:()=>{},removeEventListener:()=>{}})as unknown as MediaQueryList;});
describe('full native plugin UI',()=>{
 it('shows real empty library and navigates to settings without AI',async()=>{
  const snapshot=createPreviewSnapshot();snapshot.projects=[];snapshot.downloads=[];
  api.dispatch.mockResolvedValue(snapshot);
  const target=document.createElement('div');document.body.append(target);const app=mount(App,{target});await settle();await settle();
  expect(target.textContent).toContain('添加软件');expect(target.textContent).not.toContain('AI 连接');
  const settings=Array.from(target.querySelectorAll('button')).find(button=>button.textContent?.trim()==='设置')!;settings.click();await settle();
  expect(target.textContent).toContain('自动检查间隔');expect(target.textContent).not.toContain('托盘');
  await unmount(app);target.remove();
 });
 it('renders a backend error honestly rather than substituting demo data',async()=>{
  api.dispatch.mockRejectedValue(new Error('原生服务未连接'));const target=document.createElement('div');document.body.append(target);const app=mount(App,{target});await settle();await settle();
  expect(target.textContent).toContain('原生服务未连接');expect(target.textContent).not.toContain('FFmpegFreeUI');await unmount(app);target.remove();
 });
});
