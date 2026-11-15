<script lang="ts">
  import type { AssetPreference, Channel, Project, ProjectChanges } from '../lib/types';
  import { formatBytes, formatDate, projectInitials, projectStatus } from '../lib/ui';
  import Icon from './Icon.svelte';
  import ReleaseNotes from './ReleaseNotes.svelte';
  let { project, busy, onclose, oncheck, ondownload, onupdate, onmark, onremove, onopen, onversions }: {
    project: Project; busy: boolean; onclose: () => void; oncheck: (ids: string[]) => void;
    ondownload: (project: Project) => void; onupdate: (id: string, changes: ProjectChanges) => void;
    onmark: (project: Project) => void;
    onremove: (project: Project) => void; onopen: (url: string) => void; onversions: (projectId: string) => void;
  } = $props();
  let tab = $state<'notes' | 'assets' | 'manage'>('notes');
  let previousId = $state<string | null>(null);
  $effect(() => { if (previousId !== project.id) { previousId = project.id; tab = 'notes'; } });
  const status = $derived(projectStatus[project.status]);
  const selectedAsset = $derived(project.release?.assets.find((asset) => asset.id === project.selectedAssetId && asset.compatible));
</script>

<aside class="detail-panel" aria-label={`${project.name} 详情`}>
  <div class="detail-topline"><span>软件详情</span><button class="icon-button" aria-label="关闭软件详情" onclick={onclose}><Icon name="close" size={18} /></button></div>
  <div class="detail-identity"><span class="app-avatar large" class:avatar-purple={project.repo.toLowerCase() === 'kazumi'} class:avatar-blue={project.repo.toLowerCase() === 'apkstart'}>{projectInitials(project)}</span><h2>{project.name}</h2><button class="text-button muted" onclick={() => onopen(project.url)}>{project.owner}/{project.repo}<Icon name="external" size={13} /></button>{#if project.description}<p>{project.description}</p>{/if}<span class={`status-badge ${status.tone}`}><span class="tiny-dot"></span>{status.label}</span></div>
  <div class="version-summary"><div><span>已记录版本</span><strong>{project.installedVersion ?? '尚未记录'}</strong><small>{project.installedSource === 'managed' ? '由追新管理' : project.installedSource === 'manual' ? '手动确认' : '不自动推测本机版本'}</small></div><Icon name="arrowRight" size={18} /><div><span>最新发行 <em class:preview-label={project.release?.prerelease}>{project.release?.prerelease ? '预览' : '正式'}</em></span><strong class="accent-text">{project.release?.tag ?? '暂无信息'}</strong><small>{formatDate(project.release?.publishedAt ?? null)}</small></div></div>
  <div class="detail-primary-actions"><button class="button primary" disabled={busy || !selectedAsset} onclick={() => ondownload(project)}><Icon name="download" size={16} />下载{project.release ? '此版本' : '安装包'}</button><button class="button secondary" disabled={busy} aria-label={`检查 ${project.name} 更新`} onclick={() => oncheck([project.id])}><Icon name="refresh" size={16} /></button></div>
  {#if project.installedSource === 'managed'}<div class="managed-entry"><button class="button secondary small" disabled={busy} onclick={() => onversions(project.id)}><Icon name="drive" size={15} />管理已准备版本</button><span>打开程序目录，或确认切换当前记录</span></div>{/if}
  {#if project.error}<div class="detail-error"><Icon name="alert" size={16} /><span>{project.error}</span></div>{/if}
  {#if project.release && !selectedAsset}<button class="asset-needed" onclick={() => tab = 'assets'}><Icon name="info" size={16} /><span>还没有确定合适的安装包，查看后手动选择。</span><Icon name="chevronRight" size={14} /></button>{/if}
  <div class="detail-tabs" role="tablist" aria-label="详情内容"><button role="tab" aria-selected={tab === 'notes'} class:active={tab === 'notes'} onclick={() => tab = 'notes'}>更新说明</button><button role="tab" aria-selected={tab === 'assets'} class:active={tab === 'assets'} onclick={() => tab = 'assets'}>安装包 <span>{project.release?.assets.length ?? 0}</span></button><button role="tab" aria-selected={tab === 'manage'} class:active={tab === 'manage'} onclick={() => tab = 'manage'}>管理</button></div>
  <div class="detail-content">
    {#if tab === 'notes'}
      {#if project.release}<div class="release-title"><span>{project.release.name || project.release.tag}</span><button class="icon-button" title="在 GitHub 阅读完整说明" aria-label="在 GitHub 阅读完整说明" onclick={() => onopen(project.release!.url)}><Icon name="external" size={15} /></button></div><ReleaseNotes body={project.release.body} repositoryUrl={project.release.url} {onopen} />{:else}<div class="empty-state compact"><Icon name="notes" size={30} /><h3>{project.status === 'noRelease' ? '作者还没有发布发行版' : '还没有发行信息'}</h3><p>检查更新以获取作者正式发布的版本。源码提交不会被当作发行版。</p><button class="button secondary small" disabled={busy} onclick={() => oncheck([project.id])}>检查发行动态</button></div>{/if}
      {#if project.sourceAhead}<div class="source-callout"><Icon name="code" size={17} /><div><strong>源码有尚未发行的变化</strong><p>这还不是可下载的发行版。请等待作者发布，或到 GitHub 查看源码变化。</p><button class="text-button" onclick={() => onopen(project.url)}>查看原项目<Icon name="external" size={13} /></button></div></div>{/if}
    {:else if tab === 'assets'}
      <p class="field-hint">完整保留作者上传的文件。仅可选择适合当前系统与架构 的安装包，不确定时由你决定。</p>
      <div class="asset-list">
        {#each project.release?.assets ?? [] as asset (asset.id)}<button class="asset-choice" class:chosen={project.selectedAssetId === asset.id} disabled={!asset.compatible || busy} aria-pressed={project.selectedAssetId === asset.id} onclick={() => onupdate(project.id, { selectedAssetId: asset.id })}><span class="asset-choice-icon"><Icon name={project.selectedAssetId === asset.id ? 'success' : 'package'} size={19} /></span><span><strong>{asset.name}</strong><small>{formatBytes(asset.size)} · {asset.kind === 'portable' ? '免安装包' : asset.kind === 'installer' ? '安装程序' : '其他文件'}</small><em>{asset.reason || (asset.compatible ? '请核对文件名与平台' : '不适用于此设备')}</em></span>{#if !asset.compatible}<Icon name="cancel" size={15} />{/if}</button>{:else}<div class="empty-state compact"><Icon name="package" size={27} /><h3>没有可下载的发行文件</h3><p>作者可能只发布了源码。你可以前往原项目查看，追新不会执行源码。</p></div>{/each}
      </div>
      {#if selectedAsset}<div class="asset-integrity"><Icon name="shield" size={15} /><span>{selectedAsset.digest ? '下载后会比对上游提供的摘要，结果见下载页。' : '此文件未提供上游摘要。下载后会计算 SHA-256，但不等于发布者认证。'}</span></div>{/if}
    {:else}
      <div class="detail-settings"><label>订阅渠道<select value={project.channel} disabled={busy} onchange={(event) => onupdate(project.id, { channel: event.currentTarget.value as Channel })}><option value="stable">正式版 · 更稳妥</option><option value="preview">包含预览版 · 更早体验</option></select></label><label>安装包偏好<select value={project.preference} disabled={busy} onchange={(event) => onupdate(project.id, { preference: event.currentTarget.value as AssetPreference })}><option value="auto">自动推荐，歧义时手选</option><option value="portable">优先免安装包</option><option value="installer">优先安装程序</option></select></label><p class="field-hint">更改渠道或偏好后会重新检查。预览版也必须由作者明确发布。</p>
      <form onsubmit={(event) => { event.preventDefault(); const value = String(new FormData(event.currentTarget).get('pattern') ?? '').trim(); onupdate(project.id, { assetPattern: value || null }); }}><label>文件名筛选 <span class="optional">可选</span><input name="pattern" value={project.assetPattern ?? ''} placeholder="例如 win-x64" maxlength="200" /></label><div class="form-inline-end"><button class="button secondary small" disabled={busy} type="submit">保存筛选</button></div></form>
      <div class="detail-divider"></div><div class="setting-mini"><strong>本机版本记录</strong><p>{project.installedPath ?? '没有记录安装位置'}</p><button class="button secondary small" onclick={() => onmark(project)}><Icon name="check" size={14} />手动记录已安装版本</button></div><div class="detail-divider"></div><button class="text-button danger-text" disabled={busy} onclick={() => onremove(project)}><Icon name="trash" size={15} />取消订阅此软件</button><p class="field-hint">只移出列表，不删除下载文件和已安装的软件。</p></div>
    {/if}
  </div>
  <div class="detail-footer"><Icon name="clock" size={13} />最近检查 · {formatDate(project.checkedAt, true)}</div>
</aside>

<style>
  .managed-entry { display: flex; flex-direction: column; gap: 6px; margin: 0 23px 14px; }
  .managed-entry span { color: var(--muted); font-size: 10px; text-align: center; }
</style>
