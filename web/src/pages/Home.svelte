<script lang="ts">
  import type { Project, Snapshot } from '../lib/types';
  import { formatDate, type Page } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import ProjectList from '../components/ProjectList.svelte';
  let { snapshot, busy, selectedId, onpage, onselect, onadd, oncheck, ondownload }: {
    snapshot: Snapshot; busy: boolean; selectedId: string | null;
    onpage: (page: Page) => void; onselect: (project: Project) => void; onadd: () => void;
    oncheck: (ids?: string[]) => void; ondownload: (project: Project) => void;
  } = $props();
  const updates = $derived(snapshot.projects.filter((project) => project.status === 'updateAvailable'));
  const recent = $derived([...snapshot.projects].sort((a, b) => (b.checkedAt ?? b.createdAt).localeCompare(a.checkedAt ?? a.createdAt)).slice(0, 5));
  const activeDownloads = $derived(snapshot.downloads.filter((download) => ['queued', 'downloading'].includes(download.status)).length);
  const pendingInstalls = $derived(snapshot.downloads.filter((download) => download.status === 'completed').length);
  const failures = $derived(snapshot.projects.filter((project) => project.status === 'error').length);
  const lastCheck = $derived(snapshot.projects.reduce<string | null>((latest, project) => project.checkedAt && (!latest || project.checkedAt > latest) ? project.checkedAt : latest, null));
</script>

<div class="page-heading"><div><div class="eyebrow">YOUR UPDATE WORKSPACE</div><h1>每一次更新，都心中有数<span class="heading-dot">。</span></h1><p>把喜欢的软件放在一起，及时发现新版本，按自己的节奏更新。</p></div><span class="date-label">{new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric', weekday: 'long' }).format(new Date())}</span></div>

<section class="overview-hero" aria-labelledby="overview-title">
  <div class="hero-copy"><div class="hero-kicker"><span class="status-dot"></span>你的软件近况</div><h2 id="overview-title">{#if updates.length}<span>{updates.length}</span> 款软件，有了新变化{:else if snapshot.projects.length}好用的软件，保持常新{:else}从第一个 GitHub 项目开始{/if}</h2><p>{#if updates.length}先看看更新说明，再决定要不要升级。{#if pendingInstalls}还有 {pendingInstalls} 个安装包已准备好。{/if}{:else if snapshot.projects.length}随时检查发行动态。下载与安装分开，每一步都由你确认。{:else}粘贴项目链接，我们帮你留意正式版和预览版的发布。{/if}</p><div class="hero-actions"><button class="button primary" disabled={busy || !snapshot.projects.length} onclick={() => oncheck()}><Icon name={busy ? 'loader' : 'refresh'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在处理' : '检查全部更新'}</button><button class="button hero-secondary" onclick={updates.length ? () => onpage('updates') : onadd}>{updates.length ? '查看可更新软件' : '添加软件'}<Icon name="arrowRight" size={16} /></button></div></div>
  <div class="hero-illustration" aria-hidden="true"><div class="orbit orbit-one"></div><div class="orbit orbit-two"></div><div class="hero-package"><Icon name="package" size={48} strokeWidth={1.25} /></div><span class="hero-chip chip-check"><Icon name="check" size={15} />源自 GitHub</span><span class="hero-chip chip-release"><span class="tiny-dot"></span>发行动态 · 一处掌握</span></div>
</section>

<div class="stat-grid">
  <button class="stat-card" onclick={() => onpage('library')}><span class="stat-icon"><Icon name="library" size={20} /></span><span><span class="stat-label">已订阅软件</span><strong>{snapshot.projects.length}<small>款</small></strong></span><Icon name="arrowUpRight" size={15} /></button>
  <button class="stat-card" onclick={() => onpage('updates')}><span class="stat-icon mint"><Icon name="updates" size={20} /></span><span><span class="stat-label">等待更新</span><strong>{updates.length}<small>款</small></strong></span><Icon name="arrowUpRight" size={15} /></button>
  <button class="stat-card" onclick={() => onpage('downloads')}><span class="stat-icon warm"><Icon name="download" size={20} /></span><span><span class="stat-label">下载进行中</span><strong>{activeDownloads}<small>项</small></strong></span><Icon name="arrowUpRight" size={15} /></button>
</div>

<section class="surface recent-section" aria-labelledby="recent-title"><div class="section-heading"><div><h2 id="recent-title">软件动态 <span class="subtle-count">{snapshot.projects.length}</span></h2><p>{lastCheck ? `最近检查 · ${formatDate(lastCheck, true)}` : '添加软件后，检查一次就能看到发行动态'}</p></div><button class="text-button" onclick={() => onpage('library')}>全部软件<Icon name="chevronRight" size={15} /></button></div>
  {#if recent.length}<ProjectList projects={recent} {selectedId} {busy} {onselect} {ondownload} {oncheck} />{:else}<div class="empty-state"><span class="empty-icon"><Icon name="library" size={28} /></span><h3>给喜欢的软件，留一个位置</h3><p>支持公开 GitHub 项目，不登录也能开始。</p><button class="button primary small" onclick={onadd}><Icon name="plus" size={16} />添加第一个软件</button></div>{/if}
  {#if failures}<div class="section-notice"><Icon name="alert" size={16} /><span>{failures} 款软件检查失败，先看看错误原因再重试。</span><button class="text-button" onclick={() => onpage('library')}>查看</button></div>{/if}
</section>

<div class="home-bottom-grid"><button class="quiet-card" onclick={() => onpage('settings')}><span class="quiet-icon"><Icon name="drive" size={24} /></span><span><strong>你的软件库，随身带走</strong><small>订阅与设置可迁移，凭据只留在这台电脑。</small></span><Icon name="chevronRight" size={17} /></button><button class="quiet-card" onclick={() => onpage('github')}><span class="quiet-icon"><Icon name="github" size={24} /></span><span><strong>GitHub 仓库，集中管理</strong><small>浏览 Stars、自己的仓库与近期动态。</small></span><Icon name="chevronRight" size={17} /></button></div>
<p class="page-footnote"><Icon name="shield" size={13} />不自动安装，不改动已有软件。每一次更新，由你确认。</p>
