<script lang="ts">
  import type { GithubRepository, GithubRepositoryDetail } from '../lib/types';
  import { formatDate, safeGithubUrl } from '../lib/ui';
  import Icon from './Icon.svelte';
  import ReleaseNotes from './ReleaseNotes.svelte';
  let { name, detail, loading, error, connected, preview, pending, subscribed, adding, onclose, onretry, onstar, onedit, onadd, onopen }: {
    name: string; detail: GithubRepositoryDetail | null; loading: boolean; error: string;
    connected: boolean; preview: boolean; pending: boolean; subscribed: boolean; adding: boolean;
    onclose: () => void; onretry: () => void; onstar: (repository: GithubRepository) => void;
    onedit: (repository: GithubRepository) => void; onadd: (repository: GithubRepository) => void; onopen: (url: string) => void;
  } = $props();
  const repository = $derived(detail?.repository ?? null);
</script>

<aside class="surface github-detail" aria-label={`${name} 仓库详情`} aria-busy={loading}>
  <div class="repo-detail-top"><button class="text-button" onclick={onclose}><Icon name="chevronRight" class="back-arrow" size={15} />返回列表</button><span>仓库详情</span><button class="icon-button" aria-label="刷新仓库详情" disabled={loading || pending} onclick={onretry}><Icon name="refresh" size={16} /></button></div>
  {#if loading}
    <div class="detail-body" role="status"><p class="muted">正在读取 {name}…</p><div class="skeleton-lines"><span></span><span></span><span></span></div></div>
  {:else if error}
    <div class="empty-state"><Icon name="alert" size={26} /><h3>仓库详情暂时没有读到</h3><p class="break-word" role="alert">{error}</p><button class="button secondary" onclick={onretry}>重新读取</button></div>
  {:else if repository}
    <div class="detail-body">
      <div class="repo-heading"><Icon name="readme" size={21} /><h2>{repository.fullName}</h2></div>
      <div class="repo-flags">{#if repository.private}<span class="status-badge neutral"><Icon name="private" size={12} />私有</span>{:else}<span class="status-badge neutral">公开</span>{/if}{#if repository.archived}<span class="status-badge warning">已归档</span>{/if}{#if repository.fork}<span class="status-badge neutral">Fork</span>{/if}{#if preview}<span class="status-badge warning">示例仓库</span>{/if}</div>
      <p class="repo-description">{repository.description || '作者尚未填写仓库说明。'}</p>
      {#if repository.homepage}<div class="repo-homepage"><strong>主页</strong>{#if safeGithubUrl(repository.homepage)}<button class="text-button" onclick={() => onopen(repository.homepage!)}>{repository.homepage}<Icon name="external" size={13} /></button>{:else}<span>{repository.homepage}</span><small>站外地址仅作展示；可到 GitHub 原页查看。</small>{/if}</div>{/if}
      <div class="repo-statline"><span><Icon name="star" size={14} />{repository.stars.toLocaleString()} Stars</span><span><Icon name="fork" size={14} />{repository.forks.toLocaleString()} Forks</span>{#if repository.language}<span>{repository.language}</span>{/if}<span>更新于 {formatDate(repository.updatedAt)}</span></div>
      {#if repository.topics.length}<div class="repo-topics">{#each repository.topics as topic}<span>{topic}</span>{/each}</div>{/if}
      <div class="repo-actions">
        <button class="button secondary" disabled={!connected || repository.starred === null || pending || preview} onclick={() => onstar(repository)}><Icon name={pending ? 'loader' : 'star'} class={pending ? 'spinning' : ''} size={15} />{pending ? '正在保存' : !connected ? '登录后可 Star' : repository.starred === null ? 'Star 状态未知' : repository.starred ? '取消 Star' : 'Star'}</button>
        <button class="button primary" disabled={preview || repository.private || subscribed || adding} onclick={() => onadd(repository)}><Icon name={adding ? 'loader' : subscribed ? 'check' : 'plus'} class={adding ? 'spinning' : ''} size={15} />{subscribed ? '已订阅' : adding ? '正在加入' : '加入软件库'}</button>
        {#if repository.canAdmin && connected}<button class="button secondary" disabled={pending} onclick={() => onedit(repository)}><Icon name="edit" size={15} />修改说明与主页</button>{/if}
      </div>
      <p class="field-hint">{repository.private ? '私有仓库暂不加入更新列表。' : '加入软件库只登记订阅，不自动下载或安装。'}{#if connected && repository.starred === null} Star 状态未读到，请刷新详情后再操作。{/if}</p>
      <button class="text-button original-link" onclick={() => onopen(repository.url)}>在 GitHub 查看原页<Icon name="external" size={14} /></button>
      <div class="readme-heading"><Icon name="notes" size={16} /><h3>README</h3><span>已过滤不安全内容</span></div>
      {#if detail?.readme.trim()}<ReleaseNotes body={detail.readme} repositoryUrl={`${repository.url}/blob/${encodeURIComponent(repository.defaultBranch)}/README.md`} {onopen} />{:else}<p class="empty-readme">该仓库没有可展示的 README，可到 GitHub 原页阅读。</p>{/if}
    </div>
  {/if}
</aside>

<style>
  .github-detail { min-width: 0; align-self: start; }
  .repo-detail-top { display: flex; align-items: center; gap: 12px; padding: 11px 16px; border-bottom: 1px solid var(--border); }
  .repo-detail-top > span { margin-left: auto; color: var(--muted); font-size: 11px; }
  .repo-detail-top :global(.back-arrow) { transform: rotate(180deg); }
  .detail-body { padding: 18px; min-width: 0; }
  .repo-heading { display: flex; align-items: flex-start; gap: 10px; color: var(--accent); }
  .repo-heading > :global(svg) { margin-top: 3px; }
  h2 { font-size: 16px; line-height: 1.5; overflow-wrap: anywhere; }
  .repo-flags { display: flex; flex-wrap: wrap; gap: 6px; margin: 10px 0; }
  .repo-description { color: var(--subtle); font-size: 12px; overflow-wrap: anywhere; line-height: 1.8; }
  .repo-homepage { display: flex; flex-direction: column; align-items: start; gap: 3px; margin-top: 12px; font-size: 12px; overflow-wrap: anywhere; }
  .repo-homepage strong, .repo-homepage small { color: var(--muted); font-size: 11px; font-weight: 400; }
  .repo-homepage .text-button { white-space: normal; overflow-wrap: anywhere; display: block; }
  .repo-statline { display: flex; flex-wrap: wrap; gap: 7px 16px; color: var(--muted); font-size: 11px; margin: 15px 0 10px; font-variant-numeric: tabular-nums; }
  .repo-statline > span { display: inline-flex; align-items: center; gap: 4px; }
  .repo-topics { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0; }
  .repo-topics span { padding: 1px 7px; border-radius: 4px; background: var(--surface-soft); border: 1px solid var(--border); color: var(--subtle); font-size: 11px; overflow-wrap: anywhere; }
  .repo-actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 17px 0 8px; }
  .original-link { margin-top: 10px; }
  .readme-heading { display: flex; align-items: center; gap: 7px; margin: 24px 0 13px; padding-bottom: 10px; border-bottom: 1px solid var(--border); color: var(--muted); }
  .readme-heading h3 { font-size: 12px; font-weight: 600; }
  .readme-heading span { margin-left: auto; font-size: 10.5px; }
  .empty-readme { color: var(--muted); font-size: 12px; }
</style>
