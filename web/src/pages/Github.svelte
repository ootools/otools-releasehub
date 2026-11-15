<script lang="ts">
  import { onMount, tick } from 'svelte';
  import GithubLoginOptions from '../components/GithubLoginOptions.svelte';
  import { dispatch } from '../lib/bridge';
  import { GITHUB_DEVICE_URL, filterGithubPage, githubAccountKey, isSubscribed, repositoryEditError } from '../lib/github';
  import type { GithubActivityPage, GithubRepository, GithubRepositoryDetail, GithubRepositoryPage, GithubRepositoryTab, GithubStatus, Project } from '../lib/types';
  import { formatDate, readableError, type Tone } from '../lib/ui';
  import Icon from '../components/Icon.svelte';
  import Modal from '../components/Modal.svelte';
  import GithubAvatar from '../components/GithubAvatar.svelte';
  import GithubDetail from '../components/GithubDetail.svelte';
  import type { GithubPreviewScenario } from '../lib/github-preview';

  let { projects, preview, onadd, onopen, oncopy, onnotice }: {
    projects: Project[]; preview: boolean;
    onadd: (repository: GithubRepository) => Promise<boolean>;
    onopen: (url: string) => void; oncopy: (text: string) => void; onnotice: (message: string, tone?: Tone) => void;
  } = $props();
  type Tab = GithubRepositoryTab | 'activity';
  const tabs = [
    { id: 'explore', label: '搜索仓库', icon: 'search' },
    { id: 'owned', label: '我的仓库', icon: 'user' },
    { id: 'starred', label: 'Stars', icon: 'star' },
    { id: 'activity', label: '动态', icon: 'activity' },
  ] as const;
  let tab = $state<Tab>('explore');
  let searchText = $state('windows');
  let searchQuery = $state('windows');
  let page = $state(1);
  let status = $state<GithubStatus | null>(null);
  let statusLoading = $state(true);
  let statusError = $state('');
  let authBusy = $state(false);
  let repositories = $state<GithubRepositoryPage | null>(null);
  let activity = $state<GithubActivityPage | null>(null);
  let contentLoading = $state(false);
  let contentError = $state('');
  let selectedName = $state<string | null>(null);
  let detail = $state<GithubRepositoryDetail | null>(null);
  let detailLoading = $state(false);
  let detailError = $state('');
  let pendingRepositories = $state(new Set<string>());
  let addingRepositories = $state(new Set<string>());
  let editing = $state<GithubRepository | null>(null);
  let description = $state('');
  let homepage = $state('');
  let editConfirmed = $state(false);
  let editBusy = $state(false);
  let editError = $state('');
  let previewScenario = $state<GithubPreviewScenario>('connected');
  let detailRegion = $state<HTMLDivElement>();
  let previousFocus: HTMLElement | null = null;
  let destroyed = false;
  let sessionRevision = 0;
  let statusRequest = 0;
  let listRequest = 0;
  let detailRequest = 0;
  let loginTimer: ReturnType<typeof setTimeout> | undefined;
  const connected = $derived(Boolean(status?.connected && status.account));
  const filtered = $derived(tab === 'explore' ? repositories?.items ?? [] : filterGithubPage(repositories?.items ?? [], searchText));
  const hasMore = $derived(tab === 'activity' ? activity?.hasMore ?? false : repositories?.hasMore ?? false);
  const waiting = $derived(status?.login?.state === 'waiting');

  function stopLoginPoll() { if (loginTimer !== undefined) clearTimeout(loginTimer); loginTimer = undefined; }
  function scheduleLoginPoll() {
    stopLoginPoll();
    if (!destroyed && !preview && !authBusy && status?.login?.state === 'waiting') {
      loginTimer = setTimeout(() => { loginTimer = undefined; void refreshStatus(); }, 2_000);
    }
  }
  function clearAccountContent(resetFilters = true) {
    sessionRevision += 1; listRequest += 1; detailRequest += 1;
    repositories = null; activity = null; contentError = ''; contentLoading = false;
    selectedName = null; detail = null; detailError = ''; detailLoading = false; previousFocus = null;
    editing = null; description = ''; homepage = ''; editConfirmed = false; editError = ''; editBusy = false;
    pendingRepositories = new Set(); addingRepositories = new Set();
    if (resetFilters) { page = 1; searchText = tab === 'explore' ? 'windows' : ''; searchQuery = 'windows'; }
  }
  function acceptStatus(next: GithubStatus) {
    const previousAccount = githubAccountKey(status);
    const changedAccount = previousAccount !== githubAccountKey(next);
    status = next;
    // Initial account detection must not erase a search typed while it was loading.
    if (changedAccount) { clearAccountContent(Boolean(previousAccount)); void loadContent(); }
  }
  async function refreshStatus(showLoading = false) {
    if (destroyed || authBusy) return;
    const request = ++statusRequest;
    if (showLoading) statusLoading = true;
    statusError = '';
    try {
      const next = preview && import.meta.env.DEV
        ? (await import('../lib/github-preview')).createPreviewGithubStatus(previewScenario)
        : await dispatch('github_status', {});
      if (!destroyed && request === statusRequest) acceptStatus(next);
    } catch (error) {
      if (!destroyed && request === statusRequest) statusError = readableError(error);
    } finally {
      if (!destroyed && request === statusRequest) { statusLoading = false; scheduleLoginPoll(); }
    }
  }
  async function accountAction(action: 'github_login' | 'github_cancel_login' | 'github_logout') {
    if (preview || authBusy || destroyed) return;
    const request = ++statusRequest;
    stopLoginPoll(); authBusy = true; statusError = ''; statusLoading = false;
    if (action === 'github_logout') {
      // Hide private content immediately, not after the logout round-trip.
      clearAccountContent();
      if (status) status = { ...status, connected: false, account: null, login: null };
    }
    try {
      const next = await dispatch(action, {});
      if (!destroyed && request === statusRequest) acceptStatus(next);
    } catch (error) {
      if (!destroyed && request === statusRequest) statusError = readableError(error);
    } finally {
      if (!destroyed && request === statusRequest) {
        authBusy = false; scheduleLoginPoll();
        if (!repositories && !activity && !contentLoading) void loadContent();
      }
    }
  }
  async function loadContent() {
    const request = ++listRequest;
    const session = sessionRevision;
    const requestedTab = tab;
    const requestedPage = page;
    const query = requestedTab === 'explore' ? searchQuery : '';
    repositories = null; activity = null; contentError = ''; contentLoading = false;
    if (destroyed || (requestedTab !== 'explore' && (!connected || authBusy)) || (requestedTab === 'explore' && !query)) return;
    contentLoading = true;
    const current = () => !destroyed && session === sessionRevision && request === listRequest;
    try {
      if (requestedTab === 'activity') {
        const result = preview && import.meta.env.DEV
          ? (await import('../lib/github-preview')).createPreviewGithubActivity(requestedPage, previewScenario)
          : await dispatch('github_activity', { page: requestedPage });
        if (current()) activity = result;
      } else {
        const result = preview && import.meta.env.DEV
          ? (await import('../lib/github-preview')).createPreviewGithubRepositories(requestedTab, query, requestedPage, previewScenario)
          : await dispatch('github_repositories', { tab: requestedTab, query, page: requestedPage });
        if (current()) repositories = result;
      }
    } catch (error) { if (current()) contentError = readableError(error); }
    finally { if (current()) contentLoading = false; }
  }
  function resetDetail() { detailRequest += 1; selectedName = null; detail = null; detailError = ''; detailLoading = false; }
  function changeTab(next: Tab) {
    if (tab === next) return;
    tab = next; page = 1; searchText = next === 'explore' ? 'windows' : ''; searchQuery = 'windows';
    resetDetail(); void loadContent();
  }
  function tabKey(event: KeyboardEvent, index: number) {
    const next = event.key === 'ArrowRight' ? (index + 1) % tabs.length : event.key === 'ArrowLeft' ? (index + tabs.length - 1) % tabs.length : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault(); changeTab(tabs[next].id);
    document.getElementById(`github-tab-${tabs[next].id}`)?.focus();
  }
  function search(event: SubmitEvent) {
    event.preventDefault();
    if (tab !== 'explore') return;
    searchQuery = searchText.trim(); page = 1; resetDetail(); void loadContent();
  }
  function goPage(next: number) { page = Math.max(1, next); resetDetail(); void loadContent(); }
  async function selectRepository(name: string, focus = true) {
    if (pendingRepositories.has(name)) return;
    if (focus) previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    selectedName = name; detail = null; detailError = ''; detailLoading = true;
    const request = ++detailRequest;
    const session = sessionRevision;
    const current = () => !destroyed && request === detailRequest && session === sessionRevision && selectedName === name;
    if (focus) { await tick(); if (current()) detailRegion?.focus(); }
    try {
      const result = preview && import.meta.env.DEV
        ? (await import('../lib/github-preview')).createPreviewGithubDetail(name, connected)
        : await dispatch('github_repository', { repository: name });
      if (current()) detail = result;
    } catch (error) { if (current()) detailError = readableError(error); }
    finally { if (current()) detailLoading = false; }
  }
  async function closeDetail() { resetDetail(); await tick(); if (previousFocus?.isConnected) previousFocus.focus(); }
  function setRepositoryPending(name: string, value: boolean) {
    const next = new Set(pendingRepositories); if (value) next.add(name); else next.delete(name); pendingRepositories = next;
  }
  function updateVisibleRepository(name: string, update: (repository: GithubRepository) => GithubRepository) {
    if (repositories) repositories = { ...repositories, items: repositories.items.map((item) => item.fullName === name ? update(item) : item) };
    if (detail?.repository.fullName === name) detail = { ...detail, repository: update(detail.repository) };
  }
  async function setStar(repository: GithubRepository) {
    if (preview || !connected || authBusy || repository.starred === null || pendingRepositories.has(repository.fullName)) return;
    const session = sessionRevision;
    setRepositoryPending(repository.fullName, true);
    try {
      const result = await dispatch('github_set_star', { repository: repository.fullName, starred: !repository.starred, confirmed: true });
      if (destroyed || session !== sessionRevision) return;
      const starDelta = repository.starred === result.starred ? 0 : result.starred ? 1 : -1;
      updateVisibleRepository(repository.fullName, (item) => ({ ...item, starred: result.starred, stars: Math.max(0, item.stars + starDelta) }));
      onnotice(result.starred ? `已 Star ${repository.fullName}。` : `已取消 ${repository.fullName} 的 Star。`);
      if (tab === 'starred' && !result.starred && repositories) repositories = { ...repositories, items: repositories.items.filter((item) => item.fullName !== repository.fullName) };
    } catch (error) { if (!destroyed && session === sessionRevision) onnotice(readableError(error), 'danger'); }
    finally { if (!destroyed && session === sessionRevision) setRepositoryPending(repository.fullName, false); }
  }
  async function addRepository(repository: GithubRepository) {
    if (preview || repository.private || isSubscribed(repository.fullName, projects) || addingRepositories.has(repository.fullName)) return;
    const session = sessionRevision;
    addingRepositories = new Set([...addingRepositories, repository.fullName]);
    try { await onadd(repository); }
    catch (error) { if (!destroyed && session === sessionRevision) onnotice(readableError(error), 'danger'); }
    finally { if (!destroyed && session === sessionRevision) { const next = new Set(addingRepositories); next.delete(repository.fullName); addingRepositories = next; } }
  }
  function editRepository(repository: GithubRepository) {
    if (!repository.canAdmin || !connected || authBusy || pendingRepositories.has(repository.fullName)) return;
    editing = repository; description = repository.description ?? ''; homepage = repository.homepage ?? ''; editConfirmed = false; editError = '';
  }
  async function saveRepository(event: SubmitEvent) {
    event.preventDefault();
    if (!editing || preview || !connected || authBusy || editBusy || !editing.canAdmin || !editConfirmed) return;
    editError = repositoryEditError(description, homepage) ?? '';
    if (editError) return;
    const name = editing.fullName;
    const session = sessionRevision;
    editBusy = true; setRepositoryPending(name, true);
    try {
      const result = await dispatch('github_update_repository', { repository: name, description: description.trim(), homepage: homepage.trim(), confirmed: true });
      if (destroyed || session !== sessionRevision) return;
      updateVisibleRepository(name, (item) => ({ ...item, description: result.description, homepage: result.homepage, updatedAt: result.updatedAt }));
      editing = null; onnotice('仓库说明与主页已保存到 GitHub。');
    } catch (error) { if (!destroyed && session === sessionRevision) editError = readableError(error); }
    finally { if (!destroyed && session === sessionRevision) { editBusy = false; setRepositoryPending(name, false); } }
  }
  function changePreview() {
    stopLoginPoll(); statusRequest += 1; clearAccountContent(); status = null;
    void refreshStatus(true); void loadContent();
  }
  onMount(() => {
    void refreshStatus(true); void loadContent();
    return () => { destroyed = true; statusRequest += 1; listRequest += 1; detailRequest += 1; sessionRevision += 1; stopLoginPoll(); };
  });
</script>

<div class="page-heading"><div><h1>GitHub</h1><p>浏览仓库、整理 Stars，把想跟进的软件加入更新列表。</p></div><span class="quiet-label"><Icon name="github" size={16} />日常管理</span></div>

{#if preview}
  <div class="github-preview"><span><strong>GitHub 界面示例</strong> · 账号、仓库与数量均为演示，不访问真实账号。</span><label>示例状态<select aria-label="GitHub 示例状态" bind:value={previewScenario} onchange={changePreview}><option value="connected">已连接</option><option value="anonymous">未登录</option><option value="waiting">等待短码授权</option><option value="unavailable">缺少登录组件</option><option value="empty">空列表</option><option value="error">读取失败</option></select></label></div>
{/if}

<section class="surface github-account" aria-label="GitHub 账号">
  <div class="account-line">
    <GithubAvatar url={status?.account?.avatarUrl} name={status?.account?.login ?? 'GitHub'} />
    <div class="account-identity" aria-live="polite">
      {#if statusLoading}<strong>正在读取账号状态…</strong><p>公开仓库仍可搜索和浏览。</p>
      {:else if connected && status?.account}<strong>{status.account.name || status.account.login}{#if preview}<span class="status-badge warning">示例账号</span>{/if}</strong><p>@{status.account.login} · 已连接 GitHub CLI</p>
      {:else if waiting}<strong>等待你在 GitHub 完成授权</strong><p>短码只用于这次登录；完成后本页会自动更新。</p>
      {:else if status?.available === false}<strong>没有找到 GitHub 登录组件</strong><p>公开仓库仍可查看。请使用带 GitHub CLI 的完整发布包。</p>
      {:else}<strong>连接你的 GitHub 账号</strong><p>公开仓库无需登录；我的仓库、Stars 和动态需要登录。</p>{/if}
    </div>
    <div class="account-actions"><button class="icon-button" aria-label="刷新 GitHub 账号状态" disabled={authBusy || statusLoading} onclick={() => void refreshStatus(true)}><Icon name="refresh" size={16} /></button>{#if waiting}<button class="button secondary" disabled={authBusy || preview} onclick={() => void accountAction('github_cancel_login')}>{authBusy ? '正在取消' : '取消登录'}</button>{:else if connected}<button class="button secondary" disabled={authBusy || preview} onclick={() => void accountAction('github_logout')}><Icon name="logout" size={15} />仅断开追新</button>{:else}<button class="button primary" disabled={authBusy || statusLoading || status?.available === false || preview} onclick={() => void accountAction('github_login')}><Icon name={authBusy ? 'loader' : 'github'} class={authBusy ? 'spinning' : ''} size={16} />{authBusy ? '正在连接' : '登录 GitHub'}</button>{/if}</div>
  </div>
  <p class="account-note">经 GitHub CLI 授权，包含私有仓库读写访问；本页只提供浏览、Star 和修改说明/主页等日常操作。同机 CLI 可能共用授权，断开仅退出追新，不退出其他 gh 工具。登录信息留在本机，不随便携目录迁移。</p>
  {#if waiting}<div class="device-login"><div><label for="github-device-code">一次性登录短码</label><div class="code-line"><input id="github-device-code" class="mono" readonly value={status?.login?.userCode ?? '正在获取短码…'} /><button class="button secondary" disabled={!status?.login?.userCode} onclick={() => oncopy(status!.login!.userCode!)}><Icon name="copy" size={15} />复制短码</button></div></div><div><button class="button primary" disabled={!status?.login?.userCode || preview} onclick={() => onopen(GITHUB_DEVICE_URL)}>打开 GitHub 授权页<Icon name="external" size={15} /></button><p>仅打开 github.com/login/device，请核对账号后输入短码。</p></div></div>{/if}
  {#if statusError || status?.error || status?.login?.state === 'error'}<p class="field-error" role="alert">{statusError || status?.error || status?.login?.message || '登录未完成，请重新登录。'}</p>{/if}
</section>


<GithubLoginOptions {preview} onstatus={(next)=>{acceptStatus(next);void loadContent();}} {onopen}/>
<div class="github-tabs" role="tablist" aria-label="GitHub 内容">{#each tabs as item, index}<button id={`github-tab-${item.id}`} role="tab" aria-selected={tab === item.id} aria-controls="github-results" tabindex={tab === item.id ? 0 : -1} class:active={tab === item.id} onclick={() => changeTab(item.id)} onkeydown={(event) => tabKey(event, index)}><Icon name={item.icon} size={16} />{item.label}</button>{/each}</div>

{#if tab !== 'activity'}
  <form class="github-search" onsubmit={search}><label for="github-search">{tab === 'explore' ? '搜索 GitHub 仓库' : '筛选本页'}</label><div class="search-line"><div class="input-leading"><Icon name={tab === 'explore' ? 'search' : 'filter'} size={17} /><input id="github-search" bind:value={searchText} maxlength="256" placeholder={tab === 'explore' ? '关键词、作者/仓库，或 topic:windows' : '按名称、说明或语言筛选本页'} disabled={tab !== 'explore' && !connected} />{#if searchText}<button type="button" class="icon-button" aria-label="清空仓库搜索" onclick={() => searchText = ''}><Icon name="close" size={14} /></button>{/if}</div>{#if tab === 'explore'}<button class="button primary" type="submit">搜索</button>{/if}</div><p class="field-hint">{tab === 'explore' ? '使用 GitHub 搜索语法；未登录时可搜索公开仓库。' : '仅筛选当前这一页，不是全部仓库搜索；可继续翻页查找。'}</p></form>
{:else}<p class="activity-note"><Icon name="clock" size={15} />GitHub 近期公开 / 关注活动，可能延迟，并非实时消息或完整的 Star 动态；最多展示最近 30 天、300 条。</p>{/if}

<div class="github-columns" class:with-detail={selectedName !== null}>
  <div class="surface github-results" class:compact-hidden={selectedName !== null} id="github-results" role="tabpanel" tabindex="0" aria-labelledby={`github-tab-${tab}`} aria-busy={contentLoading}>
    {#if tab !== 'explore' && !connected}
      <div class="empty-state spacious"><Icon name={tab === 'starred' ? 'star' : tab === 'owned' ? 'user' : 'activity'} size={29} /><h3>登录后查看{tab === 'starred' ? '你的 Stars' : tab === 'owned' ? '自己的仓库' : 'GitHub 动态'}</h3><p>上方连接账号后会载入这里。也可以先搜索公开仓库。</p><button class="button secondary" onclick={() => changeTab('explore')}>先浏览公开仓库</button></div>
    {:else if contentLoading}
      <div class="repo-loading" role="status"><p>正在读取{tab === 'activity' ? '近期活动' : '仓库列表'}…</p>{#each [1, 2, 3] as row}<div class="loading-row" aria-hidden="true"><span></span><div><i></i><i></i></div></div>{/each}</div>
    {:else if contentError}
      <div class="empty-state spacious"><Icon name="alert" size={27} /><h3>暂时没有读到{tab === 'activity' ? '活动' : '仓库'}</h3><p class="break-word" role="alert">{contentError}</p><button class="button secondary" onclick={() => void loadContent()}>重试这一页</button></div>
    {:else if tab === 'explore' && !searchQuery}
      <div class="empty-state spacious"><Icon name="search" size={29} /><h3>从一个关键词开始</h3><p>输入项目名称、作者或 GitHub 搜索条件，查找你想关注的软件。</p></div>
    {:else if tab === 'activity'}
      <div class="results-heading"><h2>近期活动</h2><span>第 {page} 页</span><button class="icon-button" aria-label="刷新 GitHub 活动" onclick={() => void loadContent()}><Icon name="refresh" size={15} /></button></div>
      {#each activity?.items ?? [] as item (item.id)}<article class="activity-row"><GithubAvatar url={item.avatarUrl} name={item.actor} /><div><p><strong>{item.actor}</strong> {item.summary}</p><button class="text-button repo-link" onclick={() => void selectRepository(item.repository)}>{item.repository}<Icon name="chevronRight" size={13} /></button><span>{formatDate(item.createdAt, true)}</span></div><button class="icon-button" aria-label={`在 GitHub 查看 ${item.actor} 的活动`} onclick={() => onopen(item.url)}><Icon name="external" size={15} /></button></article>{:else}<div class="empty-state"><Icon name="activity" size={27} /><h3>这一页没有近期活动</h3><p>GitHub 不一定会立即提供动态，可稍后刷新，或返回上一页。</p></div>{/each}
    {:else}
      <div class="results-heading"><h2>{tab === 'explore' ? '搜索结果' : tab === 'owned' ? '我的仓库' : '我的 Stars'}</h2><span>{tab === 'explore' && repositories?.totalCount !== null && repositories?.totalCount !== undefined ? `约 ${repositories.totalCount.toLocaleString()} 个结果` : `本页 ${filtered.length} / ${repositories?.items.length ?? 0} 个`}</span><button class="icon-button" aria-label="刷新仓库列表" onclick={() => void loadContent()}><Icon name="refresh" size={15} /></button></div>
      {#each filtered as repository (repository.id)}
        <article class="repo-row" class:selected={selectedName === repository.fullName}>
          <button class="repo-main" aria-label={`查看 ${repository.fullName} 详情`} aria-pressed={selectedName === repository.fullName} onclick={() => void selectRepository(repository.fullName)}><span class="repo-title"><Icon name={repository.private ? 'private' : 'readme'} size={16} /><strong>{repository.fullName}</strong>{#if repository.private}<span class="status-badge neutral">私有</span>{/if}{#if repository.archived}<span class="status-badge warning">已归档</span>{/if}</span><span class="repo-summary">{repository.description || '尚未填写仓库说明'}</span></button>
          <div class="repo-row-bottom"><div class="repo-meta"><span><Icon name="star" size={13} />{repository.stars.toLocaleString()}{#if repository.starred === true}<span class="starred-label">已 Star</span>{/if}</span>{#if repository.language}<span>{repository.language}</span>{/if}<span>更新 {formatDate(repository.updatedAt)}</span></div><button class="button secondary small" disabled={preview || repository.private || isSubscribed(repository.fullName, projects) || addingRepositories.has(repository.fullName)} title={repository.private ? '私有仓库暂不加入更新列表' : '只登记订阅，不自动下载或安装'} onclick={() => void addRepository(repository)}><Icon name={addingRepositories.has(repository.fullName) ? 'loader' : isSubscribed(repository.fullName, projects) ? 'check' : 'plus'} class={addingRepositories.has(repository.fullName) ? 'spinning' : ''} size={14} />{isSubscribed(repository.fullName, projects) ? '已订阅' : addingRepositories.has(repository.fullName) ? '正在加入' : '加入软件库'}</button></div>
          {#if repository.private}<p class="private-note">私有仓库暂不加入更新列表。</p>{/if}
        </article>
      {:else}<div class="empty-state"><Icon name={tab === 'starred' ? 'star' : 'search'} size={27} /><h3>{tab !== 'explore' && searchText.trim() ? '本页没有匹配的仓库' : tab === 'starred' ? '这一页还没有 Stars' : '这一页没有找到仓库'}</h3><p>{tab !== 'explore' && searchText.trim() ? '可以清空筛选，或继续翻页；这里只筛选当前一页。' : tab === 'explore' ? '换一个关键词，或缩短搜索条件后重试。' : '可浏览公开仓库，或稍后刷新这一页。'}</p>{#if tab !== 'explore' && searchText.trim()}<button class="button secondary" onclick={() => searchText = ''}>清空本页筛选</button>{/if}</div>{/each}
    {/if}
    {#if !contentLoading && !contentError && (tab === 'explore' || connected) && (repositories || activity)}<div class="github-pagination"><span>第 {page} 页{tab !== 'explore' && tab !== 'activity' ? ' · 筛选仅限本页' : ''}</span><div><button class="button secondary small" disabled={page <= 1} onclick={() => goPage(page - 1)}>上一页</button><button class="button secondary small" disabled={!hasMore} onclick={() => goPage(page + 1)}>下一页</button></div></div>{/if}
  </div>
  {#if selectedName}<div class="detail-region" bind:this={detailRegion} tabindex="-1"><GithubDetail name={selectedName} {detail} loading={detailLoading} error={detailError} {connected} {preview} pending={pendingRepositories.has(selectedName)} subscribed={isSubscribed(selectedName, projects)} adding={addingRepositories.has(selectedName)} onclose={() => void closeDetail()} onretry={() => { if (selectedName) void selectRepository(selectedName, false); }} onstar={(repository) => void setStar(repository)} onedit={editRepository} onadd={(repository) => void addRepository(repository)} {onopen} /></div>{/if}
</div>

{#if editing}
  <Modal title="修改仓库说明与主页" description={`将直接更新 GitHub 上的 ${editing.fullName}，不会修改代码。`} onclose={() => { if (!editBusy) editing = null; }}>
    <form class="modal-form" onsubmit={saveRepository}><label>仓库说明<textarea bind:value={description} maxlength="350" rows="3" disabled={editBusy} data-initial-focus placeholder="用一句话说明这个仓库做什么；留空可移除"></textarea></label><label>主页地址 <span class="optional">可选</span><input type="url" bind:value={homepage} maxlength="2000" disabled={editBusy} placeholder="https://example.com" /></label><p class="field-hint">只支持完整 HTTPS 地址，留空会移除主页。站外地址仅展示文字，安全打开仍限 GitHub。</p><label class="confirmation-check"><input type="checkbox" bind:checked={editConfirmed} disabled={editBusy || preview} required />我确认将以上说明与主页保存到 GitHub。</label>{#if preview}<p class="field-hint">这是示例，不能提交到真实仓库。</p>{/if}{#if editError}<p class="field-error" role="alert">{editError}</p>{/if}<div class="modal-footer"><button class="button secondary" type="button" disabled={editBusy} onclick={() => editing = null}>取消</button><button class="button primary" type="submit" disabled={editBusy || !editConfirmed || preview}><Icon name={editBusy ? 'loader' : 'check'} class={editBusy ? 'spinning' : ''} size={16} />{editBusy ? '正在保存' : '确认保存到 GitHub'}</button></div></form>
  </Modal>
{/if}

<style>
  .github-preview { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 8px 20px; color: var(--warning); background: var(--warning-bg); border: 1px solid var(--border); border-radius: 6px; margin-bottom: 14px; padding: 8px 12px; font-size: 11px; }
  .github-preview label { display: flex; gap: 8px; align-items: center; }
  .github-preview select { padding: 4px 8px; font-size: 11px; }
  .github-account { padding: 16px 18px; }
  .account-line { display: flex; align-items: center; gap: 11px; }
  .account-identity { flex: 1; min-width: 0; }
  .account-identity strong { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; color: var(--heading); font-size: 13px; overflow-wrap: anywhere; }
  .account-identity p { color: var(--muted); font-size: 11.5px; margin-top: 2px; }
  .account-actions { display: flex; gap: 9px; align-items: center; flex-shrink: 0; }
  .account-note { color: var(--muted); font-size: 11px; margin-top: 13px; padding-top: 11px; border-top: 1px solid var(--border); line-height: 1.8; }
  .device-login { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 14px 22px; border-top: 1px solid var(--border); padding-top: 14px; margin-top: 14px; }
  .device-login label { display: block; font-size: 11px; color: var(--subtle); margin-bottom: 5px; }
  .code-line { display: flex; gap: 8px; align-items: center; }
  .code-line input { width: 178px; padding: 7px 10px; font-size: 17px; font-weight: 650; letter-spacing: .04em; }
  .device-login p { color: var(--muted); font-size: 11px; margin-top: 6px; }
  .github-tabs { display: flex; align-items: center; gap: 20px; margin-top: 18px; border-bottom: 1px solid var(--border); }
  .github-tabs button { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: 12px; padding: 11px 2px; border-bottom: 2px solid transparent; }
  .github-tabs button:hover { color: var(--accent); }
  .github-tabs button.active { color: var(--accent); border-bottom-color: var(--accent); font-weight: 650; }
  .github-search { margin: 16px 0; }
  .github-search > label { display: block; font-size: 11.5px; color: var(--subtle); margin-bottom: 6px; }
  .search-line { display: flex; gap: 9px; }
  .search-line .input-leading { flex: 1; background: var(--surface); }
  .search-line input { font-size: 12px; }
  .search-line .icon-button { margin-right: 4px; }
  .github-search .field-hint { margin-top: 6px; }
  .activity-note { display: flex; align-items: flex-start; gap: 7px; color: var(--muted); font-size: 11.5px; padding: 16px 0; }
  .activity-note :global(svg) { margin-top: 3px; }
  .github-columns { display: grid; grid-template-columns: minmax(0, 1fr); gap: 18px; align-items: start; }
  .github-columns.with-detail { grid-template-columns: minmax(310px, .9fr) minmax(380px, 1.1fr); }
  .github-results, .detail-region { min-width: 0; }
  .detail-region:focus { outline: none; }
  .detail-region:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; border-radius: 8px; }
  .results-heading { display: flex; align-items: center; gap: 12px; padding: 10px 16px; background: var(--surface-soft); }
  .results-heading h2 { font-size: 12px; }
  .results-heading > span { margin-left: auto; color: var(--muted); font-size: 11px; }
  .repo-row { padding: 14px 16px; border-top: 1px solid var(--border); }
  .repo-row:hover { background: var(--surface-soft); }
  .repo-row.selected { background: var(--accent-soft); }
  .repo-main { display: block; width: 100%; min-width: 0; padding: 0; text-align: left; }
  .repo-title { display: flex; align-items: center; gap: 7px; color: var(--accent); flex-wrap: wrap; }
  .repo-title strong { font-size: 13px; font-weight: 600; overflow-wrap: anywhere; }
  .repo-summary { display: block; color: var(--subtle); font-size: 12px; line-height: 1.7; margin-top: 5px; overflow-wrap: anywhere; }
  .repo-row-bottom { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 9px; margin-top: 11px; }
  .repo-meta { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 13px; color: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .repo-meta > span { display: inline-flex; gap: 4px; align-items: center; }
  .starred-label { color: var(--accent); margin-left: 3px; }
  .private-note { color: var(--muted); font-size: 11px; margin-top: 7px; }
  .repo-loading { padding: 18px; color: var(--muted); font-size: 12px; }
  .loading-row { display: flex; align-items: center; gap: 12px; border-top: 1px solid var(--border); padding: 20px 0; margin-top: 12px; }
  .loading-row > span { width: 32px; height: 32px; background: var(--surface-soft); border-radius: 6px; }
  .loading-row > div { flex: 1; display: flex; flex-direction: column; gap: 9px; }
  .loading-row i { height: 8px; border-radius: 3px; width: 75%; background: var(--border); }
  .loading-row i:last-child { width: 48%; }
  .activity-row { display: flex; align-items: flex-start; gap: 11px; padding: 15px 16px; border-top: 1px solid var(--border); }
  .activity-row > div { flex: 1; min-width: 0; }
  .activity-row p { font-size: 12px; color: var(--subtle); overflow-wrap: anywhere; }
  .activity-row strong { color: var(--heading); }
  .repo-link { max-width: 100%; overflow-wrap: anywhere; font-size: 12px; }
  .activity-row div > span { display: block; color: var(--muted); font-size: 11px; margin-top: 3px; }
  .github-pagination { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 12px 16px; border-top: 1px solid var(--border); font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .github-pagination > div { display: flex; gap: 8px; }
  @media (max-width: 1250px) { .github-columns.with-detail { grid-template-columns: minmax(0, 1fr); } .github-results.compact-hidden { display: none; } }
  @media (max-width: 1040px) { .account-line { flex-wrap: wrap; } .account-actions { margin-left: 45px; } }
</style>
