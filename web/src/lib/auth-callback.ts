import type { DeepLinksApi, PluginCallback, PluginDeepLink } from 'otools-plugin-sdk/deep-links';
interface Dependencies {
  deepLinks: DeepLinksApi;
  connect(token: string): Promise<unknown>;
  open(url: string): Promise<void>;
  status(value: unknown): void;
  error(message: string): void;
}
export function createGithubCallbackLogin(dependencies: Dependencies) {
  let pending: PluginCallback | undefined;
  let unlisten: (() => void) | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  const clear = () => { pending=undefined;if(timer)clearTimeout(timer);timer=undefined; };
  const receive = async (entry: PluginDeepLink) => {
    if(entry.action!=='callback'||entry.route!=='github-login'||!pending)return;
    const state=entry.params.state;
    if(!state||state.length!==1||state[0]!==pending.state)return;
    const callback=pending;clear();
    if(Date.now()/1000>=callback.expiresAt){dependencies.error('授权已过期，请重新登录。');return;}
    if(entry.params.error){dependencies.error('GitHub 授权未完成，请重试。');return;}
    const tokens=entry.params.token||entry.params.access_token;
    if(!tokens||tokens.length!==1||!/^[-\w.]{8,4096}$/.test(tokens[0])){dependencies.error('授权回调没有有效令牌，请检查授权服务。');return;}
    try{const status=await dependencies.connect(tokens[0]);dependencies.status(status);}catch{dependencies.error('GitHub 授权验证或安全保存失败，请重新登录。');}
  };
  return {
    async start(service: string) {
      let url:URL;
      try{url=new URL(service);}catch{throw new Error('请输入有效的 HTTPS 授权服务地址。');}
      if(url.protocol!=='https:'||url.username||url.password||url.hash)throw new Error('授权服务必须使用无账户信息的 HTTPS 地址。');
      const current=++generation;
      if(pending)await dependencies.deepLinks.cancelCallback(pending.state);
      clear();
      if(!unlisten)unlisten=await dependencies.deepLinks.onOpen(receive);
      const callback=await dependencies.deepLinks.createCallback({route:'github-login',openMode:'panel',ttlSeconds:300});
      if(current!==generation){await dependencies.deepLinks.cancelCallback(callback.state);return;}
      pending=callback;
      url.searchParams.set('redirect_uri',callback.url);url.searchParams.set('state',callback.state);
      timer=setTimeout(()=>{const state=pending?.state;clear();if(state){void dependencies.deepLinks.cancelCallback(state);dependencies.error('授权已过期，请重新登录。');}},Math.min(600000,Math.max(0,callback.expiresAt*1000-Date.now())));
      try{await dependencies.open(url.toString());}catch(error){clear();await dependencies.deepLinks.cancelCallback(callback.state);throw error;}
    },
    async stop(){generation++;const state=pending?.state;clear();unlisten?.();unlisten=undefined;if(state)await dependencies.deepLinks.cancelCallback(state);},
  };
}
