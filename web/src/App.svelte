<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { desktopAvailable, dispatch, watchChanges, type Action, type Actions } from './lib/bridge';
  import { applyDownloadProgress, mergeSnapshotDownloads } from './lib/download-state';
  import type { AddProjectInput, AssetPreference, Channel, DownloadRecord, GithubRepository, ManagedVersion, Project, ProjectChanges, Settings, Snapshot } from './lib/types';
  import { canActivateManagedVersion, formatBytes, parseRepository, readableError, safeGithubUrl, type Page, type Tone } from './lib/ui';
  import ManagedVersions from './components/ManagedVersions.svelte';
  import Icon from './components/Icon.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import Modal from './components/Modal.svelte';
  import ProjectDetail from './components/ProjectDetail.svelte';
  import Home from './pages/Home.svelte';
  import Library from './pages/Library.svelte';
  import Downloads from './pages/Downloads.svelte';
  import Github from './pages/Github.svelte';
  import { isSubscribed } from './lib/github';
  import SettingsPage from './pages/Settings.svelte';

  type DialogState = { kind: 'add' } | { kind: 'install'; download: DownloadRecord } | { kind: 'managed'; projectId: string } | { kind: 'mark'; project: Project } | { kind: 'remove'; project: Project } | { kind: 'import' };
  interface Toast { id: number; message: string; tone: Tone }
  let snapshot = $state<Snapshot | null>(null);
  let page = $state<Page>('home');
  let query = $state('');
  let selectedId = $state<string | null>(null);
  let preview = $state(false);
  let loading = $state(true);
  let refreshing = $state(false);
  let startupError = $state('');
  let operationError = $state('');
  let pending = $state(new Set<string>());
  let toasts = $state<Toast[]>([]);
  let modal = $state<DialogState | null>(null);
  let formError = $state('');
  let managedVersions = $state<ManagedVersion[]>([]);
  let managedLoading = $state(false);
  let managedError = $state('');
  let managedRequestId = 0;
  let searchInput = $state<HTMLInputElement>();
  let systemDark = $state(false);
  let destroyed = false;
  let refreshRequested = false;
  let refreshPromise: Promise<void> | null = null;
  let unlisten: (() => void) | undefined;
  let toastId = 0;
  const toastTimers = new Set<ReturnType<typeof setTimeout>>();
  const busy = $derived(pending.size > 0);
  const selectedProject = $derived(snapshot?.projects.find((project) => project.id === selectedId) ?? null);
  const managedProjectId = $derived(modal?.kind === 'managed' ? modal.projectId : null);
  const managedProject = $derived(snapshot?.projects.find((project) => project.id === managedProjectId) ?? null);
  const showingDetails = $derived(selectedProject !== null && ['home', 'library', 'updates'].includes(page));
  const pageLabels: Record<Page, string> = { home: '首页', library: '软件库', updates: '可更新', downloads: '下载', github: 'GitHub', settings: '设置' };

  let addRepository = $state('');
  let addChannel = $state<Channel>('stable');
  let addPreference = $state<AssetPreference>('auto');
  let addVersion = $state('');
  let addPath = $state('');
  let addAdvanced = $state(false);
  let markVersion = $state('');
  let markPath = $state('');
  let markConfirmed = $state(false);
  let installConfirmed = $state(false);
  let importContents = $state('');
  const parsedRepository = $derived(parseRepository(addRepository));

  $effect(() => {
    const theme = snapshot?.settings.theme ?? 'system';
    const resolved = theme === 'system' ? systemDark ? 'dark' : 'light' : theme;
    document.documentElement.dataset.theme = resolved;
    document.documentElement.style.colorScheme = resolved;
    void window.otools?.setTempHostTheme?.(resolved, {background:resolved==='dark'?'#141719':'#f6f7f8'}).catch(()=>{});
  });

  function notify(message: string, tone: Tone = 'success') {
    if (destroyed) return;
    const id = ++toastId;
    toasts = [...toasts.slice(-3), { id, message, tone }];
    const timer = setTimeout(() => { toasts = toasts.filter((toast) => toast.id !== id); toastTimers.delete(timer); }, tone === 'danger' ? 9_000 : 5_500);
    toastTimers.add(timer);
  }
  function setPending(key: string, value: boolean) { const next = new Set(pending); if (value) next.add(key); else next.delete(key); pending = next; }
  function previewNotice() { const message = '这是界面预览，未执行真实操作。请在 OTools 追新插件中使用此功能。'; if (modal) formError = message; notify(message, 'warning'); }
  function reportError(error: unknown) { const message = readableError(error); operationError = message; if (modal) formError = message; notify(message, 'danger'); }

  async function refreshSnapshot() {
    if (preview || destroyed) return;
    refreshRequested = true;
    if (refreshPromise) return refreshPromise;
    refreshing = true;
    refreshPromise = (async () => {
      while (refreshRequested && !destroyed) {
        refreshRequested = false;
        const atRequestStart = snapshot;
        const next = await dispatch('snapshot', {});
        if (!destroyed && !preview) snapshot = mergeSnapshotDownloads(snapshot, next, atRequestStart);
      }
    })();
    try { await refreshPromise; } finally { refreshPromise = null; refreshing = false; }
  }

  async function operate<A extends Action>(action: A, payload: Actions[A]['payload'], options: { refresh?: boolean; success?: string; key?: string } = {}): Promise<Actions[A]['result'] | undefined> {
    if (preview) { previewNotice(); return undefined; }
    const key = options.key ?? action;
    if (pending.has(key)) return undefined;
    setPending(key, true); formError = ''; operationError = '';
    try {
      const result = await dispatch(action, payload);
      if (options.refresh !== false) {
        try { await refreshSnapshot(); } catch (error) { operationError = `操作已处理，但状态刷新失败：${readableError(error)}。请点击右上角刷新。`; }
      }
      if (options.success) notify(options.success);
      return result;
    } catch (error) { reportError(error); return undefined; }
    finally { setPending(key, false); }
  }

  async function initialize() {
    loading = true; startupError = '';
    if (!desktopAvailable()) {
      startupError = '没有连接到 OTools 插件服务。浏览器不能检查更新、下载或安装软件。';
      loading = false; return;
    }
    try {
      await refreshSnapshot();
      if (!unlisten && !destroyed) {
        try {
          const release = await watchChanges(
            () => { void refreshSnapshot().catch(reportError); },
            (record) => {
              if (destroyed || preview || !snapshot) return true;
              const next = applyDownloadProgress(snapshot, record);
              if (!next) return false;
              snapshot = next;
              return true;
            },
          );
          if (destroyed) release(); else unlisten = release;
        } catch { notify('实时状态通知暂不可用，仍可手动刷新；进行中的任务会定期同步。', 'warning'); }
      }
    } catch (error) { startupError = readableError(error); }
    finally { loading = false; }
  }

  onMount(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    systemDark = media.matches;
    const updateSystem = (event: MediaQueryListEvent) => { systemDark = event.matches; };
    media.addEventListener('change', updateSystem);
    void initialize();
    const poll = setInterval(() => {
      // Normal changes arrive as events; polling is only a recovery path if subscribing failed.
      if (!unlisten && !preview && snapshot && !document.hidden && snapshot.downloads.some((item) => ['queued', 'downloading', 'installing'].includes(item.status))) void refreshSnapshot().catch(reportError);
    }, 5_000);
    return () => { destroyed = true; unlisten?.(); clearInterval(poll); toastTimers.forEach(clearTimeout); media.removeEventListener('change', updateSystem); };
  });

  async function enterPreview() {
    if (!import.meta.env.DEV || desktopAvailable()) return;
    const { createPreviewSnapshot } = await import('./lib/preview');
    snapshot = createPreviewSnapshot(); preview = true; startupError = ''; operationError = '';
  }
  function exitPreview() { modal = null; selectedId = null; snapshot = null; preview = false; page = 'home'; query = ''; startupError = '界面预览已退出。真实操作请使用 OTools 追新插件。'; }
  function navigate(next: Page) { page = next; query = ''; if (!['home', 'library', 'updates'].includes(next)) selectedId = null; }
  function chooseProject(project: Project) { selectedId = project.id; }
  function openProject(id: string) { if (!snapshot?.projects.some((project) => project.id === id)) { notify('这个软件已取消订阅，下载记录仍然保留。', 'warning'); return; } page = 'library'; query = ''; selectedId = id; }
  function openDialog(value: DialogState) { formError = ''; modal = value; }
  function closeDialog() { if (busy) return; modal = null; formError = ''; importContents = ''; managedRequestId += 1; managedVersions = []; managedError = ''; managedLoading = false; }
  function showAdd() { addRepository = ''; addChannel = 'stable'; addPreference = 'auto'; addVersion = ''; addPath = ''; addAdvanced = false; openDialog({ kind: 'add' }); }
  function showMark(project: Project, suggestion?: string) { markVersion = suggestion ?? project.installedVersion ?? ''; markPath = project.installedPath ?? ''; markConfirmed = false; openDialog({ kind: 'mark', project }); }
  function markDownloaded(id: string, version: string) { const project = snapshot?.projects.find((item) => item.id === id); if (project) showMark(project, version); else notify('请先重新订阅这个软件，再记录本机版本。', 'warning'); }
  function showInstall(download: DownloadRecord) { installConfirmed = false; openDialog({ kind: 'install', download }); }

  function showManagedVersions(projectId: string) {
    if (busy) return;
    const project = snapshot?.projects.find((item) => item.id === projectId);
    if (!project) { notify('这个软件已取消订阅，已准备的文件仍然保留。请先重新订阅。', 'warning'); return; }
    if (project.installedSource !== 'managed') { notify('当前没有由追新管理的安装记录。手动记录的外部软件不在这里切换。', 'warning'); return; }
    openDialog({ kind: 'managed', projectId });
    void loadManagedVersions();
  }
  async function loadManagedVersions() {
    if (modal?.kind !== 'managed' || busy) return;
    const projectId = modal.projectId;
    const requestId = ++managedRequestId;
    const current = () => !destroyed && requestId === managedRequestId && modal?.kind === 'managed' && modal.projectId === projectId;
    managedVersions = []; managedLoading = true; managedError = ''; formError = '';
    try {
      let versions: ManagedVersion[];
      if (preview && import.meta.env.DEV) {
        const { createPreviewManagedVersions } = await import('./lib/preview');
        versions = createPreviewManagedVersions(projectId);
      } else {
        versions = await dispatch('list_managed_versions', { projectId });
      }
      if (current()) managedVersions = versions;
    } catch (error) { if (current()) managedError = readableError(error); }
    finally { if (current()) managedLoading = false; }
  }
  async function activateManagedVersion(downloadId: string, confirmed: boolean): Promise<boolean> {
    if (preview) { previewNotice(); return false; }
    const project = managedProject;
    const version = managedVersions.find((item) => item.downloadId === downloadId);
    if (busy || managedLoading || managedError || !project || !canActivateManagedVersion(project, version, confirmed)) return false;
    const result = await operate('activate_managed_version', { projectId: project.id, downloadId, confirmed: true });
    if (!result) return false;
    if (snapshot) snapshot = { ...snapshot, projects: snapshot.projects.map((item) => item.id === result.id ? result : item) };
    await loadManagedVersions();
    notify(`当前版本记录已切换为 ${result.installedVersion}。没有运行程序或迁移配置。`);
    return true;
  }
  async function openManagedVersion(downloadId: string) {
    if (busy || managedLoading || managedProject?.installedSource !== 'managed' || !managedVersions.some((item) => item.downloadId === downloadId)) return;
    await operate('open_managed_version', { projectId: managedProject.id, downloadId }, { refresh: false });
  }

  async function checkUpdates(ids?: string[]) {
    const result = await operate('check_updates', { ...(ids ? { projectIds: ids } : {}), allowAutoDownload: true });
    if (result) notify(`已检查 ${result.checked} 款软件，发现 ${result.updates} 项更新${result.errors ? `，${result.errors} 项未完成，请查看软件详情` : '。'}`, result.errors ? 'warning' : 'success');
  }
  async function downloadProject(project: Project, quiet = false): Promise<boolean> {
    const asset = project.release?.assets.find((item) => item.id === project.selectedAssetId && item.compatible);
    if (!asset) { chooseProject(project); if (!quiet) notify('请先在详情的“安装包”中选择适合 当前系统 的文件。', 'warning'); return false; }
    const result = await operate('download_asset', { projectId: project.id, assetId: asset.id }, { refresh: !quiet, key: `download:${project.id}` });
    if (result && !quiet) notify(`${project.name} 已加入下载队列，完成后仍需确认安装。`);
    return result !== undefined;
  }
  async function batchDownload(projects: Project[]) {
    if (preview) { previewNotice(); return; }
    setPending('batch-download', true);
    let queued = 0;
    try { for (const project of projects) if (await downloadProject(project, true)) queued += 1; await refreshSnapshot(); notify(`已将 ${queued} 项加入下载队列${projects.length > queued ? `；另有 ${projects.length - queued} 项未下载，请检查安装包或错误提示` : '。'}`, queued === projects.length ? 'success' : 'warning'); }
    catch (error) { reportError(error); } finally { setPending('batch-download', false); }
  }
  async function updateProject(id: string, changes: ProjectChanges) { const result = await operate('update_project', { id, changes }, { success: '订阅偏好已保存。' }); if (result && ('channel' in changes || 'preference' in changes || 'assetPattern' in changes)) await checkUpdates([id]); }
  async function saveSettings(changes: Partial<Settings>) {
    if (preview && changes.theme && snapshot) { snapshot = { ...snapshot, settings: { ...snapshot.settings, theme: changes.theme } }; notify('仅切换了预览外观，没有保存真实设置。', 'neutral'); return; }
    await operate('save_settings', { settings: changes }, { success: '设置已保存。' });
  }

  async function openUrl(url: string) { const safe = safeGithubUrl(url); if (!safe) { notify('只支持在系统浏览器打开安全的 HTTPS GitHub 链接，请到项目原文查看其他地址。', 'warning'); return; } await operate('open_url', { url: safe }, { refresh: false }); }
  async function copy(text: string) { try { if(window.otools?.copyText)window.otools.copyText(text);else await navigator.clipboard.writeText(text); notify('已复制到剪贴板。'); } catch { notify('剪贴板不可用，请选择文本后按 Ctrl+C 复制。', 'warning'); } }
  async function addGithubRepository(repository: GithubRepository): Promise<boolean> {
    if (preview) { previewNotice(); return false; }
    if (repository.private) { notify('私有仓库暂不加入更新列表。', 'warning'); return false; }
    if (isSubscribed(repository.fullName, snapshot?.projects ?? [])) { notify('这个仓库已经订阅。', 'neutral'); return true; }
    const input: AddProjectInput = { repository: repository.fullName, channel: 'stable', preference: 'auto' };
    const project = await operate('add_project', { input }, { key: `github-add:${repository.fullName.toLowerCase()}`, success: '已加入软件库；没有下载或安装，请在软件库中手动检查发行版。' });
    if (project && snapshot && !snapshot.projects.some((item) => item.id === project.id)) snapshot = { ...snapshot, projects: [...snapshot.projects, project] };
    return project !== undefined;
  }
  async function exportFile() { const result = await operate('export_library_file', {}, { refresh: false }); if (result) notify(`订阅已导出到 ${result.path}，不包含密钥。`); }
  async function importFile() { const result = await operate('import_library_file', {}); if (result) notify(`已导入 ${result.imported} 项，跳过 ${result.skipped} 项；没有自动下载。`); }
  async function copyExport() { const result = await operate('export_library', {}, { refresh: false }); if (result !== undefined) await copy(result); }
  async function pickPath(target: 'add' | 'mark') { const result = await operate('pick_directory', {}, { refresh: false }); if (result) { if (target === 'add') addPath = result; else markPath = result; } }
  async function refreshView() { if (preview) { previewNotice(); return; } try { await refreshSnapshot(); operationError = ''; notify('已同步最新本机状态。'); } catch (error) { reportError(error); } }

  async function submitAdd(event: SubmitEvent) {
    event.preventDefault(); formError = '';
    const repository = parseRepository(addRepository);
    if (!repository) { formError = '请输入“作者/仓库”，或完整的 https://github.com/作者/仓库 地址。'; return; }
    const input: AddProjectInput = { repository, channel: addChannel, preference: addPreference, ...(addVersion.trim() ? { installedVersion: addVersion.trim() } : {}), ...(addPath.trim() ? { installedPath: addPath.trim() } : {}) };
    const project = await operate('add_project', { input }, { success: '已加入软件库。' });
    if (project) { modal = null; page = 'library'; query = ''; selectedId = project.id; await checkUpdates([project.id]); }
  }
  async function submitMark(event: SubmitEvent) {
    event.preventDefault(); if (modal?.kind !== 'mark' || !markConfirmed || !markVersion.trim()) return;
    const result = await operate('mark_installed', { projectId: modal.project.id, version: markVersion.trim(), ...(markPath.trim() ? { path: markPath.trim() } : {}) }, { success: '已按你的确认记录本机版本。' });
    if (result) modal = null;
  }
  async function submitInstall(event: SubmitEvent) {
    event.preventDefault(); if (modal?.kind !== 'install' || !installConfirmed) return;
    const download = modal.download;
    const result = await operate('install_download', { id: download.id, confirmed: true });
    if (result) {
      modal = null;
      notify(result.message || (result.installed ? '已安装到追新管理的目录，没有自动运行。' : '已打开安装程序。请在安装完成后手动记录版本。'), result.installed ? 'success' : 'neutral');
      if (result.kind === 'portable' && result.installed) showManagedVersions(download.projectId);
    }
  }
  async function submitImport(event: SubmitEvent) {
    event.preventDefault(); if (importContents.length > 1_000_000) { formError = '清单过大，请通过“导入订阅”选择文件。'; return; }
    const result = await operate('import_library', { contents: importContents });
    if (result) { importContents = ''; modal = null; notify(`已导入 ${result.imported} 项，跳过 ${result.skipped} 项；没有自动下载。`); }
  }
  async function removeProject(project: Project) { const result = await operate('remove_project', { id: project.id }, { success: '已取消订阅，下载文件和已安装软件保持不变。' }); if (result !== undefined) { modal = null; if (selectedId === project.id) selectedId = null; } }
  async function hotkeys(event: KeyboardEvent) { if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k' && snapshot && !modal && !document.querySelector('dialog[open]')) { event.preventDefault(); const githubSearch = page === 'github' ? document.querySelector<HTMLInputElement>('#github-search') : null; if (githubSearch) { githubSearch.focus(); return; } if (!['library', 'updates', 'downloads'].includes(page)) navigate('library'); await tick(); searchInput?.focus(); } if (event.key === 'Escape' && !modal) selectedId = null; }
</script>

<svelte:window onkeydown={hotkeys} />
<svelte:head><title>ReleaseHub 追新 · 软件更新工作空间</title><meta name="description" content="一个清晰、安全、便携优先的 GitHub 软件更新工作空间。" /></svelte:head>

{#if !snapshot}
  <main class="startup-shell"><div class="startup-brand"><span class="brand-mark"><Icon name="package" size={25} /></span><strong>ReleaseHub <span>追新</span></strong></div><section class="startup-card" aria-live="polite">{#if loading}<span class="startup-icon"><Icon name="loader" class="spinning" size={30} /></span><h1>正在打开你的工作空间</h1><p>读取软件库与本机设置，请稍候。</p><div class="skeleton-lines"><span></span><span></span><span></span></div>{:else}<span class="startup-icon"><Icon name="disconnected" size={29} /></span><h1>请在 OTools 中打开</h1><p class="startup-error">{startupError || '插件服务未连接，请重新打开追新。'}</p><button class="button secondary" onclick={() => void initialize()}><Icon name="refresh" size={16} />重新连接插件服务</button>{#if import.meta.env.DEV && !desktopAvailable()}<div class="preview-gate"><span class="status-badge warning">仅限开发环境</span><h2>只是想看看界面？</h2><p>预览使用明确标记的示例数据，不会访问 GitHub、保存凭据或执行下载与安装。</p><button class="button primary" onclick={() => void enterPreview()}>进入界面预览<Icon name="arrowRight" size={16} /></button></div>{/if}{/if}</section><p class="startup-footer">来自 GitHub 的变化，在这里从容掌握。</p></main>
{:else}
  <div class="app-shell"><Sidebar {page} {snapshot} {preview} onnavigate={navigate} onadd={showAdd} ondata={() => void operate('open_data_directory', {}, { refresh: false })} />
    <div class="workspace"><header class="topbar"><div class="breadcrumb"><span>工作空间</span><Icon name="chevronRight" size={13} /><strong>{pageLabels[page]}</strong></div><div class="global-search"><Icon name="search" size={16} /><input bind:this={searchInput} bind:value={query} aria-label={page === 'downloads' ? '搜索下载文件' : '搜索软件库、作者或仓库'} aria-keyshortcuts="Control+K Meta+K" placeholder={page === 'downloads' ? '搜索下载文件…' : '搜索软件库、作者或仓库…'} oninput={() => { if (!['library', 'updates', 'downloads'].includes(page)) page = 'library'; }} />{#if query}<button class="icon-button" aria-label="清空搜索" onclick={() => query = ''}><Icon name="close" size={13} /></button>{:else}<kbd>Ctrl K</kbd>{/if}</div><button class="icon-button topbar-refresh" disabled={busy || refreshing} aria-label="刷新本机状态" title="刷新本机状态" onclick={() => void refreshView()}><Icon name="refresh" class={refreshing ? 'spinning' : ''} size={17} /></button><span class="topbar-platform">{snapshot.runtime.platform} <strong>{snapshot.runtime.arch}</strong></span></header>
      {#if preview}<div class="preview-banner"><Icon name="monitor" size={15} /><strong>界面预览</strong><span>示例版本与状态 · 不执行检查、下载、安装或凭据保存</span><button onclick={exitPreview}>退出预览<Icon name="close" size={13} /></button></div>{/if}
      {#if operationError}<div class="operation-error" role="alert"><Icon name="alert" size={17} /><span>{operationError}</span><button class="icon-button" aria-label="收起错误提示" onclick={() => operationError = ''}><Icon name="close" size={16} /></button></div>{/if}
      <div class="workspace-content" class:with-details={showingDetails}><main class="page-content" id="main-content">
        {#if page === 'home'}<Home {snapshot} {busy} {selectedId} onpage={navigate} onselect={chooseProject} onadd={showAdd} oncheck={(ids) => void checkUpdates(ids)} ondownload={(project) => void downloadProject(project)} />
        {:else if page === 'library' || page === 'updates'}{#key page}<Library projects={snapshot.projects} updatesOnly={page === 'updates'} {query} {selectedId} {busy} onadd={showAdd} onselect={chooseProject} oncheck={(ids) => void checkUpdates(ids)} ondownload={(project) => void downloadProject(project)} onbatchdownload={(projects) => void batchDownload(projects)} />{/key}
        {:else if page === 'downloads'}<Downloads downloads={snapshot.downloads} {query} {busy} onretry={(id) => void operate('retry_download', { id }, { success: '已重新加入下载队列。' })} oncancel={(id) => void operate('cancel_download', { id }, { success: '已请求取消下载。' })} oninstall={showInstall} onfolder={(id) => void operate('open_download', { id }, { refresh: false })} onmark={markDownloaded} onproject={openProject} oncopy={(text) => void copy(text)} onversions={showManagedVersions} />
        {:else if page === 'github'}<Github projects={snapshot.projects} {preview} onadd={addGithubRepository} onopen={(url) => void openUrl(url)} oncopy={(text) => void copy(text)} onnotice={notify} />
        {:else}<SettingsPage settings={snapshot.settings} runtime={snapshot.runtime} {busy} {preview} onsave={(changes) => void saveSettings(changes)} ongithub={() => navigate('github')} ondata={() => void operate('open_data_directory', {}, { refresh: false })} onexport={() => void exportFile()} onimport={() => void importFile()} ontextimport={() => { importContents = ''; openDialog({ kind: 'import' }); }} oncopyexport={() => void copyExport()} onopen={(url) => void openUrl(url)} />{/if}
      </main>{#if showingDetails && selectedProject}<button class="detail-backdrop" aria-label="关闭软件详情" onclick={() => selectedId = null}></button><ProjectDetail project={selectedProject} {busy} onclose={() => selectedId = null} oncheck={(ids) => void checkUpdates(ids)} ondownload={(project) => void downloadProject(project)} onupdate={(id, changes) => void updateProject(id, changes)} onmark={showMark} onversions={showManagedVersions} onremove={(project) => openDialog({ kind: 'remove', project })} onopen={(url) => void openUrl(url)} />{/if}</div>
    </div>
  </div>
{/if}

{#if modal?.kind === 'add'}
  <Modal title="添加软件" description="订阅一个 GitHub 项目，从此不错过作者的发行动态。" onclose={closeDialog}><form class="modal-form" onsubmit={submitAdd}><label for="new-repository">GitHub 项目地址 <span class="required-mark">*</span></label><div class="input-leading"><Icon name="github" size={18} /><input id="new-repository" bind:value={addRepository} data-initial-focus placeholder="作者/仓库，或粘贴 GitHub 项目链接" required maxlength="500" spellcheck="false" /></div>{#if addRepository}<p class={parsedRepository ? 'field-hint success-text' : 'field-hint'}>{parsedRepository ? `将订阅 ${parsedRepository}` : '例如 Lake1059/FFmpegFreeUI；仅支持 GitHub 项目地址'}</p>{/if}<div class="suggestions"><span>快速填入</span>{#each ['Lake1059/FFmpegFreeUI', 'RoutineMiku/apkstart', 'Predidit/Kazumi'] as repository}<button type="button" onclick={() => addRepository = repository}>{repository.split('/')[1]}</button>{/each}</div><div class="form-columns"><label>订阅渠道<select bind:value={addChannel}><option value="stable">正式版 · 推荐</option><option value="preview">包含预览版</option></select></label><label>安装包偏好<select bind:value={addPreference}><option value="auto">自动推荐</option><option value="portable">优先免安装包</option><option value="installer">优先安装程序</option></select></label></div><button type="button" class="text-button advanced-toggle" aria-expanded={addAdvanced} onclick={() => addAdvanced = !addAdvanced}><Icon name="chevronDown" size={15} />{addAdvanced ? '收起本机版本记录' : '已经安装过？记录本机版本（可选）'}</button>{#if addAdvanced}<div class="advanced-fields"><label>已安装版本<input bind:value={addVersion} placeholder="例如 v1.2.3，不确定就留空" maxlength="120" /></label><label>安装位置<div class="input-with-button"><input bind:value={addPath} placeholder="仅作记录，不会扫描或改动此目录" maxlength="1000" /><button type="button" class="button secondary" disabled={busy} onclick={() => void pickPath('add')}>浏览</button></div></label><p class="field-hint">这里记录的是你已确认的版本，不会把“已下载”自动当成“已安装”。</p></div>{/if}{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><span class="field-hint"><Icon name="shield" size={13} />不登录也能订阅公开项目</span><button class="button secondary" type="button" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" type="submit" disabled={busy || !parsedRepository}><Icon name={busy ? 'loader' : 'plus'} size={16} class={busy ? 'spinning' : ''} />添加并检查</button></div></form></Modal>
{:else if modal?.kind === 'install'}
  <Modal title={modal.download.kind === 'portable' ? '确认安装到受管目录' : '确认打开安装程序'} description="只有这一次确认，才会进入实际安装步骤。" onclose={closeDialog}><form class="modal-form" onsubmit={submitInstall}><div class="confirm-package"><Icon name="package" size={28} /><div><strong>{modal.download.projectName} <span>{modal.download.tag}</span></strong><p>{modal.download.fileName}</p><small>{formatBytes(modal.download.totalBytes)}</small></div></div><div class="confirmation-details">{#if modal.download.kind === 'portable'}<p>{modal.download.fileName.toLowerCase().endsWith('.exe') ? '单文件主程序只会复制到追新管理的应用目录，不会在此步骤运行。' : '压缩包只会解压到追新管理的应用目录，不会直接覆盖你在其他位置安装的软件。'}</p><p>完成后会把这次准备的版本记为当前，但不会运行程序。其他版本与其配置仍留在原目录，不会自动复制或迁移；之后可在“管理已准备版本”里确认切回旧版。</p>{:else}<p>将启动下载的 EXE / MSI 安装程序。请在作者的安装向导中自行决定位置与选项。</p><p><strong>打开安装程序不代表安装完成。</strong>完成后可在下载页点“已自行安装？”记录版本。</p>{/if}<p class="integrity-disclaimer"><Icon name="shield" size={15} />{modal.download.verified ? '该文件与上游提供的摘要一致。' : '未获得上游摘要的一致性验证，请确认你信任该项目。'}</p></div><label class="confirmation-check"><input type="checkbox" bind:checked={installConfirmed} required />我已核对项目、版本与文件，并同意执行上述操作。</label>{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><button type="button" class="button secondary" disabled={busy} onclick={closeDialog}>暂不安装</button><button class="button primary" type="submit" disabled={busy || !installConfirmed}><Icon name={busy ? 'loader' : 'install'} class={busy ? 'spinning' : ''} size={16} />{busy ? '正在处理' : modal.download.kind === 'portable' ? '确认安装' : '确认打开安装程序'}</button></div></form></Modal>
{:else if modal?.kind === 'managed'}
  {#key modal.projectId}<ManagedVersions project={managedProject} versions={managedVersions} loading={managedLoading} error={managedError} actionError={formError} {busy} {preview} onclose={closeDialog} onrefresh={() => void loadManagedVersions()} onopen={(id) => void openManagedVersion(id)} onactivate={activateManagedVersion} />{/key}
{:else if modal?.kind === 'mark'}
  <Modal title="记录已安装版本" description={`这是一条手动确认记录，不是自动检测结果。软件：${modal.project.name}`} onclose={closeDialog}><form class="modal-form" onsubmit={submitMark}><label>实际已安装版本<input bind:value={markVersion} required maxlength="120" placeholder="例如 v1.2.3" data-initial-focus /></label><label>安装位置 <span class="optional">可选</span><div class="input-with-button"><input bind:value={markPath} placeholder="仅作记录，不修改这里的文件" maxlength="1000" /><button type="button" class="button secondary" disabled={busy} onclick={() => void pickPath('mark')}>浏览</button></div></label><label class="confirmation-check"><input type="checkbox" bind:checked={markConfirmed} required />我已确认此版本在本机完成安装，而不只是下载好了文件。</label>{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><button type="button" class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" type="submit" disabled={busy || !markConfirmed || !markVersion.trim()}>确认记录</button></div></form></Modal>
{:else if modal?.kind === 'remove'}
  <Modal title="取消订阅这个软件？" description={modal.project.name} onclose={closeDialog}><div class="modal-form"><p class="confirmation-details">只会从软件库移除这条订阅。下载记录、已下载文件和已安装的软件都不会被删除。</p>{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><button class="button secondary" disabled={busy} onclick={closeDialog}>继续订阅</button><button class="button danger" disabled={busy} onclick={() => { if (modal?.kind === 'remove') void removeProject(modal.project); }}>仅取消订阅</button></div></div></Modal>
{:else if modal?.kind === 'import'}
  <Modal title="粘贴订阅清单" description="导入追新导出的 JSON 清单。重复项目会跳过，不会自动下载或安装。" wide onclose={closeDialog}><form class="modal-form" onsubmit={submitImport}><label>订阅清单<textarea bind:value={importContents} rows="10" class="mono" placeholder="在这里粘贴 ReleaseHub 导出的完整 JSON" required maxlength="1000000" spellcheck="false" data-initial-focus></textarea></label><p class="field-hint">清单不应包含密钥。格式和大小由桌面服务再次检查，输入文字不会被当作代码执行。</p>{#if formError}<p class="field-error" role="alert">{formError}</p>{/if}<div class="modal-footer"><button type="button" class="button secondary" disabled={busy} onclick={closeDialog}>取消</button><button class="button primary" type="submit" disabled={busy || !importContents.trim()}><Icon name="import" size={16} />导入订阅</button></div></form></Modal>
{/if}

<div class="toast-stack" aria-live="polite" aria-atomic="false">{#each toasts as toast (toast.id)}<div class={`toast ${toast.tone}`} role={toast.tone === 'danger' ? 'alert' : 'status'}><Icon name={toast.tone === 'danger' ? 'alert' : toast.tone === 'warning' ? 'info' : toast.tone === 'success' ? 'success' : 'info'} size={19} /><p>{toast.message}</p><button class="icon-button" aria-label="关闭通知" onclick={() => toasts = toasts.filter((item) => item.id !== toast.id)}><Icon name="close" size={14} /></button></div>{/each}</div>
