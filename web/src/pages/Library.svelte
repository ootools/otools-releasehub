<script lang="ts">
  import type { Project } from '../lib/types';
  import { matchesProject } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import ProjectList from '../components/ProjectList.svelte';
  let { projects, updatesOnly = false, query, selectedId, busy, onadd, onselect, oncheck, ondownload, onbatchdownload }: {
    projects: Project[]; updatesOnly?: boolean; query: string; selectedId: string | null; busy: boolean;
    onadd: () => void; onselect: (project: Project) => void; oncheck: (ids?: string[]) => void;
    ondownload: (project: Project) => void; onbatchdownload: (projects: Project[]) => void;
  } = $props();
  let filter = $state('all');
  let selected = $state(new Set<string>());
  const base = $derived(projects.filter((project) => !updatesOnly || project.status === 'updateAvailable'));
  const attention = $derived(projects.filter((project) => ['error', 'noCompatibleAsset', 'versionUnknown'].includes(project.status)).length);
  const visible = $derived(base.filter((project) => matchesProject(project, query) && (filter === 'all' || (filter === 'attention' ? ['error', 'noCompatibleAsset', 'versionUnknown'].includes(project.status) : project.channel === filter))));
  const allVisibleSelected = $derived(visible.length > 0 && visible.every((project) => selected.has(project.id)));
  const chosen = $derived(projects.filter((project) => selected.has(project.id)));
  const downloadable = $derived(chosen.filter((project) => project.release?.assets.some((asset) => asset.id === project.selectedAssetId && asset.compatible)));
  $effect(() => {
    const allowed = new Set(base.map((project) => project.id));
    if ([...selected].some((id) => !allowed.has(id))) selected = new Set([...selected].filter((id) => allowed.has(id)));
  });
  function toggle(id: string) { const next = new Set(selected); if (next.has(id)) next.delete(id); else next.add(id); selected = next; }
  function toggleAll() { const next = new Set(selected); for (const project of visible) { if (allVisibleSelected) next.delete(project.id); else next.add(project.id); } selected = next; }
</script>

<div class="page-heading"><div><div class="eyebrow">{updatesOnly ? 'READY WHEN YOU ARE' : 'YOUR SOFTWARE COLLECTION'}</div><h1>{updatesOnly ? '可更新' : '软件库'}<span class="heading-count">{base.length}</span></h1><p>{updatesOnly ? '先阅读更新说明，选择合适的安装包。下载后仍由你确认安装。' : '订阅你常用的 GitHub 软件，管理版本、渠道和安装偏好。'}</p></div><button class="button primary" onclick={onadd}><Icon name="plus" size={17} />添加软件</button></div>

{#if updatesOnly && base.length}<div class="inline-callout"><Icon name="shieldCheck" size={19} /><div><strong>下载不等于安装</strong><span>这里仅获取官方发行包，之后在“下载”页面确认安装。</span></div><button class="button small secondary" disabled={busy} onclick={() => oncheck(base.map((project) => project.id))}><Icon name="refresh" size={15} />重新检查</button></div>{/if}

<section class="surface library-surface">
  <div class="list-toolbar"><div class="segmented-tabs" aria-label="软件筛选"><button class:active={filter === 'all'} onclick={() => filter = 'all'}>全部<span>{base.length}</span></button><button class:active={filter === 'stable'} onclick={() => filter = 'stable'}>正式版</button><button class:active={filter === 'preview'} onclick={() => filter = 'preview'}>预览版</button>{#if !updatesOnly}<button class:active={filter === 'attention'} onclick={() => filter = 'attention'}>需留意{#if attention}<span class="attention-count">{attention}</span>{/if}</button>{/if}</div><button class="icon-button" aria-label="检查列表中的软件" title="检查当前筛选结果" disabled={busy || !visible.length} onclick={() => oncheck(visible.map((project) => project.id))}><Icon name="refresh" class={busy ? 'spinning' : ''} size={17} /></button></div>
  {#if selected.size}<div class="bulk-toolbar"><span>已选 <strong>{selected.size}</strong> 项</span><button class="text-button" disabled={busy} onclick={() => oncheck([...selected])}><Icon name="refresh" size={14} />检查更新</button><button class="text-button" disabled={busy || !downloadable.length} onclick={() => onbatchdownload(chosen)}><Icon name="download" size={14} />下载所选{#if downloadable.length !== chosen.length}<span>（{downloadable.length} 个可用）</span>{/if}</button><button class="text-button clear-selection" onclick={() => selected = new Set()}>取消选择</button></div>{/if}
  {#if visible.length}<div class="table-heading"><input type="checkbox" aria-label="选择当前显示的所有软件" checked={allVisibleSelected} indeterminate={!allVisibleSelected && visible.some((project) => selected.has(project.id))} onchange={toggleAll} /><span>软件 / 来源</span><span>已记录 → 最新发行</span><span>状态</span><span></span></div><ProjectList projects={visible} {selectedId} {selected} selectable {busy} {onselect} ontoggle={toggle} {ondownload} {oncheck} />
  {:else}<div class="empty-state spacious"><span class="empty-icon"><Icon name={query || filter !== 'all' ? 'search' : updatesOnly ? 'success' : 'library'} size={29} /></span><h3>{query || filter !== 'all' ? '没有匹配的软件' : updatesOnly ? '当前没有待更新的软件' : '软件库还是空的'}</h3><p>{query ? `试试软件名称或“作者/仓库”，也可以缩短关键词。` : filter !== 'all' ? '换一个筛选条件，看看其他软件。' : updatesOnly ? '已订阅的软件会出现在软件库，未记录版本的软件需先确认本机版本。' : '粘贴 GitHub 项目地址，不需要账号即可订阅。'}</p>{#if !projects.length}<button class="button primary small" onclick={onadd}><Icon name="plus" size={15} />添加软件</button>{:else if filter !== 'all'}<button class="button secondary small" onclick={() => filter = 'all'}>显示全部渠道</button>{:else if updatesOnly}<button class="button secondary small" disabled={busy} onclick={() => oncheck()}><Icon name="refresh" size={15} />检查全部软件</button>{/if}</div>{/if}
  <div class="list-footer"><span>显示 {visible.length} / {base.length} 款软件</span><span><Icon name="github" size={13} />发行信息来自原项目</span></div>
</section>
<p class="page-footnote"><Icon name="info" size={13} />“预览版”按作者已发布的发行版选择，不把最新源码提交当作可安装版本。</p>
