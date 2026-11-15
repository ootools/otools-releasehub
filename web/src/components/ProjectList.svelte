<script lang="ts">
  import type { Project } from '../lib/types';
  import { projectInitials, projectStatus } from '../lib/ui';
  import Icon from './Icon.svelte';
  let { projects, selectedId = null, selected = new Set<string>(), selectable = false, busy = false, onselect, ontoggle = () => {}, ondownload, oncheck }: {
    projects: Project[]; selectedId?: string | null; selected?: Set<string>; selectable?: boolean; busy?: boolean;
    onselect: (project: Project) => void; ontoggle?: (id: string) => void;
    ondownload: (project: Project) => void; oncheck: (ids: string[]) => void;
  } = $props();
</script>

<div class="project-list" role="list" aria-label="软件列表">
  {#each projects as project (project.id)}
    {@const status = projectStatus[project.status]}
    {@const asset = project.release?.assets.find((item) => item.id === project.selectedAssetId && item.compatible)}
    <div class="project-row" class:row-selected={selectedId === project.id} class:row-checked={selected.has(project.id)} role="listitem">
      {#if selectable}<input class="row-checkbox" type="checkbox" aria-label={`选择 ${project.name}`} checked={selected.has(project.id)} onchange={() => ontoggle(project.id)} />{/if}
      <button class="project-main" onclick={() => onselect(project)} aria-label={`查看 ${project.name} 的版本与说明`}>
        <span class="app-avatar" class:avatar-purple={project.repo.toLowerCase() === 'kazumi'} class:avatar-blue={project.repo.toLowerCase() === 'apkstart'}>{projectInitials(project)}</span>
        <span class="project-identity"><strong>{project.name}<span class="channel-dot" class:preview-channel={project.channel === 'preview'} title={project.channel === 'preview' ? '订阅预览版' : '订阅正式版'}></span></strong><span>{project.owner}/{project.repo}</span></span>
      </button>
      <button class="version-cell" onclick={() => onselect(project)} aria-label={`${project.name}，已记录 ${project.installedVersion ?? '未知'}，最新 ${project.release?.tag ?? '未知'}`}>
        <span class="current-version">{project.installedVersion ?? '未记录版本'}</span>
        {#if project.release}<Icon name="arrowRight" size={13} /><span class:next-version={project.status === 'updateAvailable'}>{project.release.tag}</span>{/if}
      </button>
      <span class={`status-badge ${status.tone}`}><span class="tiny-dot"></span>{status.label}</span>
      <div class="row-actions">
        {#if project.status === 'updateAvailable' && asset}
          <button class="icon-button accent-button" aria-label={`下载 ${project.name} ${project.release?.tag}`} title="下载更新，不会自动安装" disabled={busy} onclick={() => ondownload(project)}><Icon name="download" size={17} /></button>
        {:else if project.status === 'error' || project.status === 'notChecked'}
          <button class="icon-button" aria-label={`重新检查 ${project.name}`} title="检查更新" disabled={busy} onclick={() => oncheck([project.id])}><Icon name="refresh" size={16} /></button>
        {:else}
          <button class="icon-button" aria-label={`查看 ${project.name} 详情`} title="查看详情" onclick={() => onselect(project)}><Icon name="chevronRight" size={17} /></button>
        {/if}
      </div>
    </div>
  {/each}
</div>
