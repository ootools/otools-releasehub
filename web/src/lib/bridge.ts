import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isDownloadRecord } from './download-state';
import type {
  AddProjectInput, CheckResult, DownloadRecord,
  GithubActivityPage, GithubRepository, GithubRepositoryDetail, GithubRepositoryPage, GithubRepositoryTab, GithubStatus,
  InstallResult, ManagedVersion, Project, ProjectChanges, Settings, Snapshot,
} from './types';

/** Keep every desktop operation behind the one audited host command. */
export interface Actions {
  snapshot: { payload: Record<string, never>; result: Snapshot };
  add_project: { payload: { input: AddProjectInput }; result: Project };
  update_project: { payload: { id: string; changes: ProjectChanges }; result: Project };
  remove_project: { payload: { id: string }; result: null };
  check_updates: { payload: { projectIds?: string[]; allowAutoDownload?: boolean }; result: CheckResult };
  download_asset: { payload: { projectId: string; assetId: number }; result: DownloadRecord };
  retry_download: { payload: { id: string }; result: DownloadRecord };
  cancel_download: { payload: { id: string }; result: null };
  install_download: { payload: { id: string; confirmed: true }; result: InstallResult };
  mark_installed: { payload: { projectId: string; version: string; path?: string }; result: Project };
  list_managed_versions: { payload: { projectId: string }; result: ManagedVersion[] };
  activate_managed_version: { payload: { projectId: string; downloadId: string; confirmed: true }; result: Project };
  save_settings: { payload: { settings: Partial<Settings> }; result: Settings };
  export_library: { payload: Record<string, never>; result: string };
  import_library: { payload: { contents: string }; result: { imported: number; skipped: number } };
  pick_directory: { payload: Record<string, never>; result: string | null };
  export_library_file: { payload: Record<string, never>; result: { path: string } | null };
  import_library_file: { payload: Record<string, never>; result: { imported: number; skipped: number } | null };
  open_url: { payload: { url: string }; result: null };
  open_download: { payload: { id: string }; result: null };
  open_data_directory: { payload: Record<string, never>; result: null };
  open_managed_version: { payload: { projectId: string; downloadId: string }; result: null };
  github_status: { payload: Record<string, never>; result: GithubStatus };
  github_login: { payload: Record<string, never>; result: GithubStatus };
  github_cancel_login: { payload: Record<string, never>; result: GithubStatus };
  github_logout: { payload: Record<string, never>; result: GithubStatus };
  github_repositories: { payload: { tab: GithubRepositoryTab; query: string; page: number }; result: GithubRepositoryPage };
  github_repository: { payload: { repository: string }; result: GithubRepositoryDetail };
  github_set_star: { payload: { repository: string; starred: boolean; confirmed: true }; result: { starred: boolean } };
  github_update_repository: { payload: { repository: string; description: string; homepage: string; confirmed: true }; result: GithubRepository };
  github_activity: { payload: { page: number }; result: GithubActivityPage };
  set_github_token: { payload: { token: string }; result: GithubStatus };
}

export type Action = keyof Actions;
export const desktopAvailable = (): boolean => isTauri() || (typeof window !== 'undefined' && typeof window.otools?.invokeNative === 'function');

export async function dispatch<A extends Action>(action: A, payload: Actions[A]['payload']): Promise<Actions[A]['result']> {
  if (!desktopAvailable()) {
    throw new Error('桌面服务未连接。请在 OTools 中打开追新插件；浏览器预览不能检查更新、下载或安装软件。');
  }
  const {hostAction}=await import('./host-actions');
  const host=await hostAction(action,payload);
  if(host.handled)return host.value as Actions[A]['result'];
  return invoke<Actions[A]['result']>('dispatch', { action, payload });
}

/** onDownload returns true for an applied/old event, false when its identity needs a full refresh. */
export async function watchChanges(onChange: () => void, onDownload: (record: DownloadRecord) => boolean = () => false): Promise<UnlistenFn> {
  const releases: UnlistenFn[] = [];
  let active = true;
  let changeTimer: ReturnType<typeof setTimeout> | undefined;
  const scheduleChange = () => {
    if (!active || changeTimer !== undefined) return;
    changeTimer = setTimeout(() => { changeTimer = undefined; if (active) onChange(); }, 250);
  };
  const stop = () => {
    if (!active) return;
    active = false;
    if (changeTimer !== undefined) clearTimeout(changeTimer);
    // A closing webview must not prevent the remaining listeners from being released.
    releases.splice(0).forEach((release) => { try { release(); } catch { /* Already closed. */ } });
  };
  try {
    releases.push(await listen('releasehub:changed', scheduleChange));
    releases.push(await listen<unknown>('releasehub:download', (event) => {
      if (!active) return;
      if (isDownloadRecord(event.payload) && onDownload(event.payload)) return;
      scheduleChange();
    }));
    releases.push(await listen<{updates:number}>('releasehub:notification',(event)=>{
      if(!active)return;
      const count=Number(event.payload?.updates);
      if(Number.isInteger(count)&&count>0)window.otools?.showNotification?.(`追新：发现 ${count} 个可更新的软件`);
    }));
    return stop;
  } catch (error) {
    stop();
    throw error;
  }
}
