<script lang="ts">
  import type { RuntimeInfo, Settings, Theme } from '../lib/types';
  import Icon from '../components/Icon.svelte';
  import Toggle from '../components/Toggle.svelte';
  let { settings, runtime, busy, preview, onsave, ongithub, ondata, onexport, onimport, ontextimport, oncopyexport, onopen }: {
    settings: Settings; runtime: RuntimeInfo; busy: boolean; preview: boolean;
    onsave: (changes: Partial<Settings>) => void;
    ongithub: () => void; ondata: () => void; onexport: () => void; onimport: () => void; ontextimport: () => void;
    oncopyexport: () => void; onopen: (url: string) => void;
  } = $props();
  const themes: { id: Theme; title: string; icon: 'sun' | 'moon' | 'monitor'; description: string }[] = [
    { id: 'light', title: '浅色', icon: 'sun', description: '明亮浅灰' },
    { id: 'dark', title: '深色', icon: 'moon', description: '深色板岩' },
    { id: 'system', title: '跟随系统', icon: 'monitor', description: '与系统一致' },
  ];
</script>

<div class="page-heading"><div><div class="eyebrow">MAKE IT YOURS</div><h1>设置</h1><p>按你的习惯工作。设置跟着软件库走，密钥留在这台电脑。</p></div><span class="quiet-label"><Icon name="settings" size={16} />更改后自动保存</span></div>
<div class="settings-layout">
  <section class="surface settings-section" aria-labelledby="appearance-title"><div class="settings-heading"><Icon name="sun" size={19} /><div><h2 id="appearance-title">外观</h2><p>选择舒适的工作空间</p></div></div><div class="theme-options">{#each themes as theme}<button class="theme-option" class:chosen={settings.theme === theme.id} aria-pressed={settings.theme === theme.id} disabled={busy} onclick={() => onsave({ theme: theme.id })}><div class={`theme-preview ${theme.id}`}><span class="preview-sidebar"></span><span class="preview-content"><i></i><i></i><i></i></span>{#if settings.theme === theme.id}<span class="theme-check"><Icon name="check" size={12} /></span>{/if}</div><span><Icon name={theme.icon} size={15} /><strong>{theme.title}</strong></span><small>{theme.description}</small></button>{/each}</div></section>

  <section class="surface settings-section" aria-labelledby="updates-title"><div class="settings-heading"><Icon name="refresh" size={19} /><div><h2 id="updates-title">更新与下载</h2><p>自动留意动态，安装仍由你决定</p></div></div>
    <div class="setting-row"><div><label for="check-interval">自动检查间隔</label><p>运行期间按此间隔检查已订阅软件</p></div><select id="check-interval" value={settings.checkIntervalMinutes} disabled={busy} onchange={(event) => onsave({ checkIntervalMinutes: Number(event.currentTarget.value) })}>{#if ![0, 15, 30, 60, 180, 360, 1440].includes(settings.checkIntervalMinutes)}<option value={settings.checkIntervalMinutes}>每 {settings.checkIntervalMinutes} 分钟</option>{/if}<option value="0">仅手动检查</option><option value="15">每 15 分钟</option><option value="30">每 30 分钟</option><option value="60">每 1 小时</option><option value="180">每 3 小时</option><option value="360">每 6 小时</option><option value="1440">每天</option></select></div>
    <div class="setting-row"><div><strong>桌面通知</strong><p>发现新发行版时提醒我</p></div><Toggle checked={settings.notifications} label="桌面通知" disabled={busy} onchange={() => onsave({ notifications: !settings.notifications })} /></div>
    <div class="setting-row"><div><strong>自动下载安装包</strong><p>只下载确定匹配的文件，有歧义时等待手选；绝不自动安装</p></div><Toggle checked={settings.autoDownload} label="自动下载安装包" disabled={busy} onchange={() => onsave({ autoDownload: !settings.autoDownload })} /></div>
    <div class="setting-row"><div><label for="download-limit">同时下载数量</label><p>限制并行任务，给其他工作留些带宽</p></div><select id="download-limit" value={settings.maxConcurrentDownloads} disabled={busy} onchange={(event) => onsave({ maxConcurrentDownloads: Number(event.currentTarget.value) })}>{#each [1, 2, 3, 4] as count}<option value={count}>{count} 个任务</option>{/each}</select></div>

  </section>

  <section class="surface settings-section" aria-labelledby="github-title"><div class="settings-heading"><Icon name="github" size={19} /><div><h2 id="github-title">GitHub 账号</h2><p>搜索公开仓库不需要登录。自己的仓库、Stars 与活动在 GitHub 页面管理。</p></div></div>
    <div class="setting-note"><Icon name="shieldCheck" size={17} /><p>通过官方 GitHub CLI 在浏览器授权，也可以使用仅保存在本机系统凭据库的令牌。授权包含私有仓库读写权限，不是只读授权；同机其他 GitHub CLI 工具可能共用该账号。登录信息留在本机，不随便携资料迁移。</p></div>
    <div class="settings-bottom-line"><span class="field-hint">GitHub 的 Star 与资料修改由你主动确认。</span><button class="button secondary small" onclick={ongithub}>管理 GitHub 账号<Icon name="arrowRight" size={15} /></button></div>
  </section>



  <section class="surface settings-section" aria-labelledby="data-title"><div class="settings-heading"><Icon name="drive" size={19} /><div><h2 id="data-title">数据管理</h2><p>数据保存在 OTools 的插件专属目录</p></div><span class="status-badge neutral">插件数据</span></div><div class="data-location"><Icon name="folder" size={18} /><code>{runtime.dataDir}</code><button class="button secondary small" disabled={busy} onclick={ondata}>打开目录</button></div><p class="setting-description">导出只带走订阅和偏好，不含密钥、任务或本机绝对安装路径。完整迁移历史与受管软件时，请先退出 OTools 再备份此目录；账号凭据不会迁移。</p><div class="data-actions"><button class="button secondary small" disabled={busy} onclick={onexport}><Icon name="export" size={15} />导出订阅</button><button class="button secondary small" disabled={busy} onclick={onimport}><Icon name="import" size={15} />导入订阅</button><button class="text-button" disabled={busy} onclick={ontextimport}>粘贴清单</button><button class="text-button" disabled={busy} onclick={oncopyexport}>复制清单</button></div></section>

  <section class="about-section"><span class="brand-mark small"><Icon name="package" size={20} /></span><div><strong>ReleaseHub <span>追新</span></strong><p>v{runtime.appVersion} · MIT 开源 · {runtime.platform} {runtime.arch}</p></div><button class="text-button" onclick={() => onopen('https://github.com/ootools/otools-releasehub')}>项目主页<Icon name="external" size={14} /></button></section>
</div>
