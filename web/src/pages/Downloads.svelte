<script lang="ts">
  import type { DownloadRecord } from '../lib/types';
  import { downloadPercent, downloadStatus, formatBytes, formatDate } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  let { downloads, query, busy, onretry, oncancel, oninstall, onfolder, onmark, onproject, oncopy, onversions }: {
    downloads: DownloadRecord[]; query: string; busy: boolean;
    onretry: (id: string) => void; oncancel: (id: string) => void; oninstall: (download: DownloadRecord) => void;
    onfolder: (id: string) => void; onmark: (id: string, version: string) => void;
    onproject: (id: string) => void; oncopy: (text: string) => void; onversions: (projectId: string) => void;
  } = $props();
  let filter = $state('all');
  let expanded = $state(new Set<string>());
  const active = $derived(downloads.filter((download) => ['queued', 'downloading', 'installing'].includes(download.status)).length);
  const ready = $derived(downloads.filter((download) => download.status === 'completed').length);
  const visible = $derived(downloads.filter((download) => `${download.projectName} ${download.fileName}`.toLowerCase().includes(query.toLowerCase()) && (filter === 'all' || (filter === 'active' ? ['queued', 'downloading', 'installing'].includes(download.status) : filter === 'ready' ? download.status === 'completed' : ['failed', 'cancelled'].includes(download.status)))).slice().sort((a, b) => b.createdAt.localeCompare(a.createdAt)));
  function toggle(id: string) { const next = new Set(expanded); if (next.has(id)) next.delete(id); else next.add(id); expanded = next; }
</script>

<div class="page-heading"><div><div class="eyebrow">DOWNLOADS & INSTALLATIONS</div><h1>下载<span class="heading-count">{downloads.length}</span></h1><p>下载进度、文件校验与安装确认，都在这里。</p></div><span class="quiet-label"><Icon name="shieldCheck" size={16} />仅来自 GitHub 官方发行</span></div>
<div class="inline-callout"><Icon name="info" size={19} /><div><strong>文件已下载，不代表软件已安装</strong><span>免安装文件只放入追新管理的目录；安装程序仅在你确认后打开。</span></div></div>
<section class="surface downloads-surface"><div class="list-toolbar"><div class="segmented-tabs" aria-label="下载状态筛选"><button class:active={filter === 'all'} onclick={() => filter = 'all'}>全部</button><button class:active={filter === 'active'} onclick={() => filter = 'active'}>进行中 <span>{active}</span></button><button class:active={filter === 'ready'} onclick={() => filter = 'ready'}>待安装 <span>{ready}</span></button><button class:active={filter === 'issues'} onclick={() => filter = 'issues'}>失败 / 已取消</button></div></div>
  {#each visible as download (download.id)}
    {@const status = download.status === 'installed' && download.kind === 'portable' ? { label: '已准备 · 未自动运行', tone: 'success' } : downloadStatus[download.status]}
    {@const running = ['queued', 'downloading', 'installing'].includes(download.status)}
    <article class="download-item"><div class="download-main"><span class="download-icon" class:download-ready={['completed', 'installed'].includes(download.status)}><Icon name={download.kind === 'portable' ? 'package' : 'download'} size={23} /></span><div class="download-info"><div class="download-name"><button class="plain-link" onclick={() => onproject(download.projectId)}>{download.projectName}</button><span class="version-pill">{download.tag}</span><span class={`status-badge ${status.tone}`}>{status.label}</span></div><p class="filename" title={download.fileName}>{download.fileName}</p><div class="download-meta"><span>{download.kind === 'portable' ? download.fileName.toLowerCase().endsWith('.exe') ? '单文件免安装' : '免安装压缩包' : download.kind === 'installer' ? '安装程序' : '其他文件'}</span><span>·</span><span>{download.totalBytes > 0 ? formatBytes(download.totalBytes) : '大小待确认'}</span><span>·</span><span>{formatDate(download.createdAt, true)}</span></div></div></div>
      {#if running}<div class="download-progress"><progress aria-label={`${download.projectName} 下载进度`} max="100" value={download.totalBytes > 0 ? downloadPercent(download) : undefined}></progress><div><span>{download.status === 'installing' ? '正在处理安装，请勿关闭应用' : download.status === 'queued' ? '已加入队列，等待可用下载位置' : `${formatBytes(download.receivedBytes)} / ${download.totalBytes > 0 ? formatBytes(download.totalBytes) : '总大小未知'}`}</span><strong>{download.totalBytes > 0 && download.status === 'downloading' ? `${downloadPercent(download)}%` : ''}</strong></div></div>{/if}
      {#if download.error}<div class="download-error"><Icon name="alert" size={15} /><p>{download.error}</p></div>{/if}
      <div class="download-bottom"><div class="download-integrity">{#if download.verified}<span class="success-text"><Icon name="shieldCheck" size={14} />与上游摘要一致</span>{:else if download.sha256}<span><Icon name="shield" size={14} />已计算 SHA-256 · 未经上游摘要验证</span>{:else}<span><Icon name="clock" size={14} />{running ? '完成下载后校验' : '未记录上游校验结果'}</span>{/if}{#if download.sha256}<button class="text-button" onclick={() => toggle(download.id)}>{expanded.has(download.id) ? '收起' : '详情'}<Icon name="chevronDown" size={12} /></button>{/if}</div><div class="download-actions">
        {#if ['queued', 'downloading'].includes(download.status)}<button class="button secondary small" disabled={busy} onclick={() => oncancel(download.id)}><Icon name="cancel" size={14} />取消</button>{/if}
        {#if ['failed', 'cancelled'].includes(download.status)}<button class="button secondary small" disabled={busy} onclick={() => onretry(download.id)}><Icon name="retry" size={14} />重新下载</button>{/if}
        {#if ['completed', 'installed'].includes(download.status)}<button class="icon-button" aria-label={`打开 ${download.projectName} 的下载位置`} title="打开文件所在目录，不会运行文件" disabled={busy} onclick={() => onfolder(download.id)}><Icon name="folder" size={17} /></button>{/if}
        {#if download.status === 'completed' && download.kind !== 'other'}<button class="button primary small" disabled={busy} onclick={() => oninstall(download)}><Icon name="install" size={14} />{download.kind === 'portable' ? '安装到受管目录' : '打开安装程序'}</button>{/if}
        {#if download.status === 'installed' && download.kind === 'portable'}<button class="button secondary small" disabled={busy} onclick={() => onversions(download.projectId)}><Icon name="drive" size={14} />管理已准备版本</button>{/if}
        {#if download.status === 'completed' && download.kind === 'installer'}<button class="text-button" disabled={busy} onclick={() => onmark(download.projectId, download.tag)}>已自行安装？</button>{/if}
      </div></div>
      {#if expanded.has(download.id)}<div class="hash-details"><span>SHA-256</span><code>{download.sha256}</code><button class="icon-button" aria-label="复制 SHA-256" onclick={() => oncopy(download.sha256 ?? '')}><Icon name="copy" size={15} /></button></div>{/if}
    </article>
  {:else}<div class="empty-state spacious"><span class="empty-icon"><Icon name="download" size={29} /></span><h3>{query ? '没有匹配的下载' : filter !== 'all' ? '这里暂时没有任务' : '还没有下载记录'}</h3><p>{filter === 'ready' ? '完成下载的安装包会出现在这里，安装前会再次确认。' : '去软件库选择一个发行版本，下载进度会保留在这里。'}</p>{#if filter !== 'all'}<button class="button secondary small" onclick={() => filter = 'all'}>查看全部下载</button>{/if}</div>{/each}
</section>
<p class="page-footnote"><Icon name="shield" size={13} />计算出的 SHA-256 用来辨识文件；只有与作者提供的摘要一致，才会显示“与上游摘要一致”。</p>
