<script lang="ts">
  import { onMount } from 'svelte';
  import { dispatch } from '../lib/bridge';
  import { createGithubCallbackLogin } from '../lib/auth-callback';
  import type { GithubStatus } from '../lib/types';
  import type { DeepLinksApi } from 'otools-plugin-sdk/deep-links';
  let {preview, onstatus, onopen}: {preview:boolean;onstatus:(value:GithubStatus)=>void;onopen:(url:string)=>void}=$props();
  let expanded=$state(false), token=$state(''), service=$state(''), busy=$state(false), error=$state(''), notice=$state('');
  let callback:ReturnType<typeof createGithubCallbackLogin>|undefined;
  onMount(()=>()=>{void callback?.stop();token='';});
  async function connect(){
    if(preview||busy||!token.trim())return;busy=true;error='';
    const value=token.trim();token='';
    try{onstatus(await dispatch('set_github_token',{token:value}));expanded=false;notice='账号已连接，令牌仅保存在系统凭据库。';}
    catch{error='令牌验证或安全保存失败，请检查权限和系统凭据库。';}finally{busy=false;}
  }
  async function authorize(){
    if(preview||busy)return;busy=true;error='';notice='';
    try{
      const deepLinks=window.otools?.deepLinks as DeepLinksApi|undefined;
      if(!deepLinks)throw new Error('请使用支持安全回调的新版 OTools。');
      callback??=createGithubCallbackLogin({deepLinks,connect:(token)=>dispatch('set_github_token',{token}),open:async(url)=>{await dispatch('open_url',{url});},status:(value)=>{onstatus(value as GithubStatus);notice='GitHub 授权成功。';},error:(message)=>{error=message;notice='';}});
      await callback.start(service);notice='已打开授权页，完成后会自动返回追新面板。';
    }catch(reason){error=reason instanceof Error?reason.message:'无法打开授权服务。';}finally{busy=false;}
  }
</script>
<div class="github-login-options">
  <button class="text-button" disabled={preview} onclick={()=>expanded=!expanded} aria-expanded={expanded}>令牌登录 / 外部浏览器授权</button>
  {#if expanded}
    <div class="surface login-options-card">
      <label for="github-pat">GitHub 访问令牌</label>
      <div class="input-with-button"><input id="github-pat" type="password" bind:value={token} autocomplete="off" placeholder="github_pat_… / ghp_…" maxlength="4096" disabled={busy||preview}/><button class="button secondary small" disabled={busy||preview||!token.trim()} onclick={()=>void connect()}>连接</button></div>
      <p class="field-hint">只使用你授予的权限，保存在本机系统凭据库，不进入订阅导出。</p>
      <label for="github-oauth-service">外部 OAuth 授权服务（可选）</label>
      <div class="input-with-button"><input id="github-oauth-service" type="url" bind:value={service} placeholder="https://你的授权服务/github/authorize" disabled={busy||preview}/><button class="button secondary small" disabled={busy||preview||!service.trim()} onclick={()=>void authorize()}>浏览器授权</button></div>
      <p class="field-hint">服务需接受 redirect_uri 与 state，完成后通过 OTools 回调返回 token。无需此服务也能使用上方设备授权。</p>
    </div>
  {/if}
  {#if error}<p class="field-error" role="alert">{error}</p>{/if}
  {#if notice}<p class="field-hint" role="status">{notice}</p>{/if}
</div>
<style>
  .github-login-options{margin:0 0 20px}.login-options-card{padding:18px;display:grid;gap:10px;margin-top:10px}.login-options-card label{font-size:13px;font-weight:600}.input-with-button{display:flex;gap:8px}.input-with-button input{flex:1;min-width:0}.field-hint{margin:0}
</style>
