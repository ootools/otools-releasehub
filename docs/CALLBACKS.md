# 插件打开与授权回调

OTools 复用系统注册的 `otools://`，不为每个插件注册新的 scheme。

- `otools://plugin/<uuid>/open`：在 TABS 打开。
- `otools://plugin/<uuid>/panel`：在 Panel 打开。
- `otools://plugin/<uuid>/detach`：在独立窗口打开。
- 以上链接支持 query 和 fragment 参数，多值保留为字符串数组；`source=market/dev-debug/...` 可指定插件来源。参数不会拼进插件页面 URL，也不会保存到窗口配置。

```ts
const stop = await window.otools.deepLinks.onOpen(async entry => {
  // entry = { requestId, action, route, params: Record<string, string[]> }
  if (entry.action === 'callback' && entry.route === 'github-login') {
    // 插件仍需校验自己发起登录时记下的 state，验证 token 的账号/权限。
    await finishLogin(entry.params);
  }
});

const callback = await window.otools.deepLinks.createCallback({
  route: 'github-login', openMode: 'panel', ttlSeconds: 300,
});
// { url: 'otools://plugin/<uuid>/callback/github-login?state=<随机值>', state, expiresAt }
// expiresAt 是 Unix 秒；不要输出 URL/state/token 到日志。
// 将完整 callback.url 作为授权服务 redirect_uri，另传 callback.state 作为 OAuth state。
// 服务通过 query 或 fragment 返回 state 和 token / code 等业务参数。

await window.otools.deepLinks.cancelCallback(callback.state); // 取消或页面销毁时
stop();
```

SDK 也导出 `createDeepLinksApi` 与相关类型，普通插件优先直接使用 `window.otools.deepLinks`。

## 安全与生命周期

- 授权回调只接受宿主为真实调用插件签发的随机、来源绑定、路由绑定、单次使用 state。过期、伪造、重放或错插件拒绝。
- 参数只进入有界、10 分钟内过期的内存队列；全局事件/主窗口只接收脱敏路由，插件只收到空的定向敲门通知，再以真实 webview + 当前文档/插件来源身份取件。
- UUID 相同但来源不同的插件不能互相取件；Panel、TABS、独立窗口分开。不会因 iframe 自报 UUID 授权。
- `onOpen` 先订阅再取队列，覆盖插件冷启动和监听尚未就绪的竞态；普通打开链接可冷启动宿主。
- 登录 state 不持久化。宿主进程退出后必须重新授权，避免重启后接受旧登录链接；页面退出时应主动 cancelCallback。
- query/fragment 合计最多 64 对，键最多 64 bytes、值最多 8192 bytes、URL 最多 32 KiB；不允许控制字符、模糊路由、编码后的分隔符或脚本执行。
- 推荐传短期 authorization code 再由插件换 token。直接传 token 也可，但外部授权服务与浏览器历史仍需自行保护；宿主从不记录原始参数 URL。

原生 desktop 的 OS scheme 已注册。CLI/纯网页模式没有 OS URL 激活，浏览器 iframe 不提供此凭据回调 API，需使用原生桌面宿主。
