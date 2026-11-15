import { describe, expect, it } from 'vitest';
import { createGithubCallbackLogin } from './auth-callback';
describe('GitHub callback authentication', () => {
  it('validates service URL and host state before exchanging a token', async () => {
    let handler: ((entry: any) => void | Promise<void>) | undefined;
    const tokens: string[] = [];
    const completed: string[] = [];
    const login = createGithubCallbackLogin({
      deepLinks: {
        createCallback: async () => ({url:'otools://plugin/app/callback/github-login?state=nonce',state:'nonce',expiresAt:Math.floor(Date.now()/1000)+300}),
        cancelCallback: async () => {},
        onOpen: async callback => { handler=callback;return ()=>{handler=undefined;}; },
      },
      connect: async token => { tokens.push(token);return {connected:true}; },
      open: async url => { completed.push(url); },
      status: () => {},error: () => {},
    });
    await login.start('https://auth.example.com/github/authorize');
    const url=new URL(completed[0]);expect(url.searchParams.get('state')).toBe('nonce');
    expect(url.searchParams.get('redirect_uri')).toBe('otools://plugin/app/callback/github-login?state=nonce');
    await handler!({requestId:'forged',action:'callback',route:'github-login',params:{state:['wrong'],token:['forgedtoken']}});
    expect(tokens).toEqual([]);
    await handler!({requestId:'valid',action:'callback',route:'github-login',params:{state:['nonce'],token:['ghp_validtoken']}});
    expect(tokens).toEqual(['ghp_validtoken']);
    await handler!({requestId:'replay',action:'callback',route:'github-login',params:{state:['nonce'],token:['ghp_validtoken']}});
    expect(tokens).toHaveLength(1);
    await login.stop();
  });
  it('refuses insecure authorization URLs without issuing a capability', async () => {
    let grants=0;
    const login=createGithubCallbackLogin({deepLinks:{createCallback:async()=>{grants++;throw new Error('unexpected');},cancelCallback:async()=>{},onOpen:async()=>()=>{}},connect:async()=>({}),open:async()=>{},status:()=>{},error:()=>{}});
    await expect(login.start('http://auth.example.com')).rejects.toThrow('HTTPS');expect(grants).toBe(0);
  });
});
