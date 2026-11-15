import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { applyDownloadProgress } from './download-state';
import { createPreviewSnapshot } from './preview';
import type { DownloadRecord } from './types';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke, isTauri: mocks.isTauri }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
import { dispatch, watchChanges } from './bridge';

beforeEach(() => {
  mocks.invoke.mockReset(); mocks.isTauri.mockReset(); mocks.listen.mockReset();
  mocks.isTauri.mockReturnValue(true);
});

describe('single desktop command', () => {
  it('uses the dispatch action/payload envelope, not ad-hoc commands', async () => {
    mocks.invoke.mockResolvedValue(null);
    await dispatch('cancel_download', { id: 'download-1' });
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('dispatch', { action: 'cancel_download', payload: { id: 'download-1' } });
  });
  it('preserves the shared payload shapes for project, settings and build operations', async () => {
    mocks.invoke.mockResolvedValue(null);
    const input = { repository: 'owner/repo', channel: 'stable', preference: 'portable' } as const;
    await dispatch('add_project', { input });
    await dispatch('update_project', { id: 'project-1', changes: { selectedAssetId: 123 } });
    await dispatch('save_settings', { settings: { autoDownload: false } });
    await dispatch('check_updates', { projectIds: ['project-1'], allowAutoDownload: true });
    await dispatch('download_asset', { projectId: 'project-1', assetId: 123 });
    await dispatch('mark_installed', { projectId: 'project-1', version: 'v1.2.3', path: 'C:\\Apps\\example' });
    expect(mocks.invoke.mock.calls).toEqual([
      ['dispatch', { action: 'add_project', payload: { input } }],
      ['dispatch', { action: 'update_project', payload: { id: 'project-1', changes: { selectedAssetId: 123 } } }],
      ['dispatch', { action: 'save_settings', payload: { settings: { autoDownload: false } } }],
      ['dispatch', { action: 'check_updates', payload: { projectIds: ['project-1'], allowAutoDownload: true } }],
      ['dispatch', { action: 'download_asset', payload: { projectId: 'project-1', assetId: 123 } }],
      ['dispatch', { action: 'mark_installed', payload: { projectId: 'project-1', version: 'v1.2.3', path: 'C:\\Apps\\example' } }],
    ]);
  });
  it('reads managed versions without inventing an active version or directory', async () => {
    const versions = [{ downloadId: 'download-1', version: 'v1.2.3', path: 'C:\\ReleaseHub\\data\\apps\\project-1\\v1.2.3', active: false }];
    mocks.invoke.mockResolvedValue(versions);
    expect(await dispatch('list_managed_versions', { projectId: 'project-1' })).toEqual(versions);
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('dispatch', { action: 'list_managed_versions', payload: { projectId: 'project-1' } });
  });
  it('sends only project/download identity and explicit confirmation for managed actions', async () => {
    mocks.invoke.mockResolvedValue(null);
    await dispatch('activate_managed_version', { projectId: 'project-1', downloadId: 'download-1', confirmed: true });
    await dispatch('open_managed_version', { projectId: 'project-1', downloadId: 'download-1' });
    expect(mocks.invoke.mock.calls).toEqual([
      ['dispatch', { action: 'activate_managed_version', payload: { projectId: 'project-1', downloadId: 'download-1', confirmed: true } }],
      ['dispatch', { action: 'managed_path', payload: { projectId: 'project-1', downloadId: 'download-1' } }],
    ]);
  });
  it('never simulates managed-directory reads, switching, or opening without a desktop host', async () => {
    mocks.isTauri.mockReturnValue(false);
    await expect(dispatch('list_managed_versions', { projectId: 'project-1' })).rejects.toThrow('桌面服务未连接');
    await expect(dispatch('activate_managed_version', { projectId: 'project-1', downloadId: 'download-1', confirmed: true })).rejects.toThrow('桌面服务未连接');
    await expect(dispatch('open_managed_version', { projectId: 'project-1', downloadId: 'download-1' })).rejects.toThrow('桌面服务未连接');
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
  it('preserves a real managed-version rejection rather than returning success', async () => {
    mocks.invoke.mockRejectedValue(new Error('已准备的程序目录不存在'));
    await expect(dispatch('activate_managed_version', { projectId: 'project-1', downloadId: 'download-1', confirmed: true })).rejects.toThrow('已准备的程序目录不存在');
  });
  it('retains the explicit installer confirmation in the host request', async () => {
    mocks.invoke.mockResolvedValue({ installed: false, message: 'installer opened', kind: 'installer', path: 'test.exe' });
    const result = await dispatch('install_download', { id: 'download-1', confirmed: true });
    expect(mocks.invoke).toHaveBeenCalledWith('dispatch', { action: 'install_download', payload: { id: 'download-1', confirmed: true } });
    expect(result.installed).toBe(false);
  });
  it('rejects missing desktop services even in the test development environment', async () => {
    mocks.isTauri.mockReturnValue(false);
    await expect(dispatch('snapshot', {})).rejects.toThrow('桌面服务未连接');
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
  it('passes real host failures through rather than returning demo success', async () => {
    mocks.invoke.mockRejectedValue(new Error('网络请求失败'));
    await expect(dispatch('check_updates', { allowAutoDownload: true })).rejects.toThrow('网络请求失败');
  });
});

describe('download progress subscriptions', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function captureEvents() {
    const handlers = new Map<string, (event: { payload: unknown }) => void>();
    mocks.listen.mockImplementation(async (name: string, handler: (event: { payload: unknown }) => void) => {
      handlers.set(name, handler);
      return () => { handlers.delete(name); };
    });
    return handlers;
  }

  it('applies 100 progress events without rereading the library or replacing unrelated data', async () => {
    const handlers = captureEvents();
    const initial = createPreviewSnapshot();
    const base = initial.downloads[0];
    let current = initial;
    const changed = vi.fn();
    const progress = vi.fn((record: DownloadRecord) => {
      const next = applyDownloadProgress(current, record);
      if (!next) return false;
      current = next;
      return true;
    });
    const release = await watchChanges(changed, progress);
    for (let count = 1; count <= 100; count++) {
      handlers.get('releasehub:download')!({ payload: {
        ...base, status: 'downloading', receivedBytes: count, totalBytes: 1000,
        updatedAt: new Date(Date.parse(base.updatedAt) + count * 400).toISOString(),
      } });
    }
    await vi.advanceTimersByTimeAsync(1000);
    expect(progress).toHaveBeenCalledTimes(100);
    expect(changed).not.toHaveBeenCalled();
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(current.downloads[0].receivedBytes).toBe(100);
    expect(current.projects).toBe(initial.projects);
    expect(current.settings).toBe(initial.settings);
    expect(current.runtime).toBe(initial.runtime);
    release();
  });

  it('coalesces metadata, malformed payloads and unknown downloads into one refresh', async () => {
    const handlers = captureEvents();
    const changed = vi.fn();
    const progress = vi.fn(() => false);
    const release = await watchChanges(changed, progress);
    const unknown = { ...createPreviewSnapshot().downloads[0], id: 'unknown-record' };
    for (let count = 0; count < 20; count++) {
      handlers.get('releasehub:download')!({ payload: count % 2 ? unknown : { status: 'invalid' } });
      handlers.get('releasehub:changed')!({ payload: {} });
    }
    expect(changed).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(250);
    expect(changed).toHaveBeenCalledOnce();
    expect(progress).toHaveBeenCalledTimes(10);
    release();
  });

  it('cleans pending refreshes and ignores late notifications after unmount', async () => {
    const handlers = captureEvents();
    const changed = vi.fn();
    const progress = vi.fn(() => true);
    const release = await watchChanges(changed, progress);
    const metadataHandler = handlers.get('releasehub:changed')!;
    const downloadHandler = handlers.get('releasehub:download')!;
    metadataHandler({ payload: {} });
    release(); release();
    downloadHandler({ payload: createPreviewSnapshot().downloads[0] });
    metadataHandler({ payload: {} });
    await vi.advanceTimersByTimeAsync(1000);
    expect(changed).not.toHaveBeenCalled();
    expect(progress).not.toHaveBeenCalled();
    expect(handlers.size).toBe(0);
  });
});

describe('live state subscription cleanup', () => {
  it('releases already-opened listeners if the next listener fails', async () => {
    const release = vi.fn();
    mocks.listen.mockResolvedValueOnce(release).mockRejectedValueOnce(new Error('event unavailable'));
    await expect(watchChanges(() => {})).rejects.toThrow('event unavailable');
    expect(release).toHaveBeenCalledOnce();
  });
  it('returns one cleanup for both supported event types', async () => {
    const releaseA = vi.fn(); const releaseB = vi.fn(); const changed = vi.fn();
    mocks.listen.mockResolvedValueOnce(releaseA).mockResolvedValueOnce(releaseB).mockResolvedValueOnce(()=>{});
    const release = await watchChanges(changed);
    expect(mocks.listen.mock.calls.map((call) => call[0])).toEqual(['releasehub:changed', 'releasehub:download', 'releasehub:notification']);
    release();
    expect(releaseA).toHaveBeenCalledOnce(); expect(releaseB).toHaveBeenCalledOnce();
  });
});

describe('OTools browser handoff and real path dialogs', () => {
  it('allows the host plugin runtime even when not a standalone Tauri app', async () => {
    mocks.isTauri.mockReturnValue(false);
    vi.stubGlobal('window', { otools: { invokeNative: () => {} } });
    mocks.invoke.mockResolvedValue({ projects: [] });
    await expect(dispatch('snapshot', {})).resolves.toEqual({ projects: [] });
    vi.unstubAllGlobals();
  });
});

describe('update notification delivery',()=>{
  it('forwards notification events to the host notification API without a snapshot refresh',async()=>{
    let received: ((event:{payload:{updates:number}})=>void)|undefined;
    const notifications:unknown[]=[];
    vi.stubGlobal('window',{otools:{showNotification:(message:string)=>{notifications.push(message);}}});
    mocks.listen.mockImplementation(async(name,handler)=>{if(name==='releasehub:notification')received=handler;return()=>{};});
    const stop=await watchChanges(()=>{});
    expect(received).toBeTypeOf('function');received!({payload:{updates:3}});
    expect(notifications).toEqual(['追新：发现 3 个可更新的软件']);
    stop();vi.unstubAllGlobals();
  });
});
