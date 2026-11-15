<script lang="ts">
  import type { Page } from '../lib/ui';
  import type { Snapshot } from '../lib/types';
  import Icon from './Icon.svelte';
  let { page, snapshot, preview, onnavigate, onadd, ondata }: {
    page: Page; snapshot: Snapshot; preview: boolean;
    onnavigate: (page: Page) => void; onadd: () => void; ondata: () => void;
  } = $props();
  const updateCount = $derived(snapshot.projects.filter((project) => project.status === 'updateAvailable').length);
  const downloadCount = $derived(snapshot.downloads.filter((download) => ['queued', 'downloading'].includes(download.status)).length);
  const pages = [
    { id: 'home', label: '首页', icon: 'home' },
    { id: 'library', label: '软件库', icon: 'library' },
    { id: 'updates', label: '可更新', icon: 'updates' },
    { id: 'downloads', label: '下载', icon: 'download' },
    { id: 'github', label: 'GitHub', icon: 'github' },
  ] as const;
</script>

<aside class="sidebar">
  <button class="brand" onclick={() => onnavigate('home')} aria-label="ReleaseHub 追新首页">
    <span class="brand-mark"><Icon name="package" size={24} strokeWidth={1.7} /></span>
    <span><strong>ReleaseHub</strong><small>追新 · 让更新更从容</small></span>
  </button>
  <div class="nav-group-label">工作空间</div>
  <nav aria-label="主导航">
    {#each pages as item}
      <button class:active={page === item.id} class="nav-item" aria-current={page === item.id ? 'page' : undefined} onclick={() => onnavigate(item.id)}>
        <Icon name={item.icon} size={19} /><span>{item.label}</span>
        {#if item.id === 'updates' && updateCount}<span class="nav-count">{updateCount}</span>{/if}
        {#if item.id === 'downloads' && downloadCount}<span class="nav-dot" aria-label={`${downloadCount} 项进行中`}></span>{/if}
      </button>
    {/each}
  </nav>
  <button class="sidebar-add" onclick={onadd}><Icon name="plus" size={17} />添加软件</button>
  <div class="sidebar-spacer"></div>
  <div class="workspace-note">
    <span class="note-icon"><Icon name="shieldCheck" size={18} /></span>
    <strong>更新由你决定</strong>
    <p>订阅发行动态，不替你自动安装。</p>
  </div>
  <button class:active={page === 'settings'} class="nav-item settings-nav" aria-current={page === 'settings' ? 'page' : undefined} onclick={() => onnavigate('settings')}><Icon name="settings" size={19} /><span>设置</span></button>
  <div class="sidebar-footer">
    <button class="storage-button" onclick={ondata} title="打开数据目录"><span class="status-dot"></span>{preview ? '界面预览' : 'OTools 工作空间'}<Icon name="chevronRight" size={13} /></button>
    <span>v{snapshot.runtime.appVersion} <span class="footer-dot">·</span> {snapshot.runtime.platform} {snapshot.runtime.arch}</span>
  </div>
</aside>
