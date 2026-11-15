import { describe, expect, it } from 'vitest';
import { canActivateManagedVersion, downloadPercent, formatBytes, matchesProject, parseRepository, readableError, safeGithubUrl } from './ui';
import { createPreviewManagedVersions, createPreviewSnapshot } from './preview';
import { applyDownloadProgress, isDownloadRecord, mergeSnapshotDownloads } from './download-state';
import type { DownloadRecord } from './types';

describe('GitHub input and host-bound links', () => {
  it('accepts a repository name and normalizes supported project links', () => {
    expect(parseRepository(' Lake1059/FFmpegFreeUI ')).toBe('Lake1059/FFmpegFreeUI');
    expect(parseRepository('https://github.com/Predidit/Kazumi.git')).toBe('Predidit/Kazumi');
    expect(parseRepository('https://github.com/RoutineMiku/apkstart/releases/tag/v0.3.2')).toBe('RoutineMiku/apkstart');
  });
  it.each([
    'https://github.com.evil.example/owner/repo', 'http://github.com/owner/repo',
    'https://user@github.com/owner/repo', 'https://github.com:8443/owner/repo',
    'https://example.com/owner/repo', 'javascript:alert(1)', 'owner/repo/extra',
    'https://github.com/owner/repo/tree/main', '',
  ])('rejects unsafe or unsupported project input: %s', (input) => {
    expect(parseRepository(input)).toBeNull();
  });
  it('forwards only credential-free HTTPS GitHub links', () => {
    expect(safeGithubUrl('/owner/repo/releases', 'https://github.com/owner/repo')).toBe('https://github.com/owner/repo/releases');
    expect(safeGithubUrl('https://github.com/owner/repo/actions/runs/123')).toBe('https://github.com/owner/repo/actions/runs/123');
    expect(safeGithubUrl('https://github.com@evil.example/a')).toBeNull();
    expect(safeGithubUrl('file:///C:/Windows/System32/cmd.exe')).toBeNull();
    expect(safeGithubUrl('#content', 'https://github.com/owner/repo')).toBeNull();
  });
});

describe('managed version confirmation', () => {
  const versions = createPreviewManagedVersions('preview-ff');
  it('requires explicit confirmation before selecting a different managed version', () => {
    expect(canActivateManagedVersion({ installedSource: 'managed' }, versions[1], false)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: 'managed' }, versions[1], true)).toBe(true);
  });
  it('does not offer switching for external, missing, or already-current versions', () => {
    expect(canActivateManagedVersion({ installedSource: 'manual' }, versions[1], true)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: null }, versions[1], true)).toBe(false);
    expect(canActivateManagedVersion(null, versions[1], true)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: 'managed' }, undefined, true)).toBe(false);
    expect(canActivateManagedVersion({ installedSource: 'managed' }, versions[0], true)).toBe(false);
  });
  it('keeps preview versions isolated and consistent with the sample installed records', () => {
    const preview = createPreviewSnapshot();
    const project = preview.projects.find((item) => item.id === 'preview-ff')!;
    expect(createPreviewManagedVersions('another-project')).toEqual([]);
    expect(versions.filter((version) => version.active)).toHaveLength(1);
    expect(project.installedSource).toBe('managed');
    expect(project.installedVersion).toBe(versions[0].version);
    expect(project.installedPath).toBe(versions[0].path);
    for (const version of versions) {
      expect(preview.downloads.find((download) => download.id === version.downloadId)).toMatchObject({ projectId: project.id, tag: version.version, status: 'installed', kind: 'portable' });
    }
    const changedPreview = createPreviewManagedVersions('preview-ff');
    changedPreview[1].active = true;
    expect(createPreviewManagedVersions('preview-ff')[1].active).toBe(false);
  });
});

describe('incremental download state', () => {
  const time = (offset: number) => new Date(Date.parse('2026-11-14T00:00:00.000Z') + offset).toISOString();
  function fixture() {
    const snapshot = createPreviewSnapshot();
    const index = snapshot.downloads.findIndex((record) => record.status === 'downloading');
    const record: DownloadRecord = { ...snapshot.downloads[index], receivedBytes: 25, totalBytes: 100, createdAt: time(-1000), updatedAt: time(0) };
    snapshot.downloads[index] = record;
    return { snapshot, record, index };
  }
  it('accepts complete records and rejects malformed events before replacing UI data', () => {
    const { record } = fixture();
    expect(isDownloadRecord(record)).toBe(true);
    for (const invalid of [null, [], {}, { ...record, projectId: '' }, { ...record, status: 'success' }, { ...record, path: 42 }, { ...record, receivedBytes: Number.NaN }, { ...record, totalBytes: -1 }, { ...record, updatedAt: 'not-a-date' }]) {
      expect(isDownloadRecord(invalid)).toBe(false);
    }
  });
  it('replaces only the matching download and preserves every other list/settings reference', () => {
    const { snapshot, record, index } = fixture();
    const progress = { ...record, receivedBytes: 50, updatedAt: time(400) };
    const next = applyDownloadProgress(snapshot, progress)!;
    expect(next.downloads[index]).toBe(progress);
    expect(next.projects).toBe(snapshot.projects);
    expect(next.settings).toBe(snapshot.settings);
    expect(next.runtime).toBe(snapshot.runtime);
    snapshot.downloads.forEach((entry, i) => { if (i !== index) expect(next.downloads[i]).toBe(entry); });
    expect(snapshot.downloads[index]).toBe(record);
    expect(record.receivedBytes).toBe(25);
  });
  it('ignores older and same-clock events without allocating a new state', () => {
    const { snapshot, record } = fixture();
    expect(applyDownloadProgress(snapshot, { ...record, receivedBytes: 10, updatedAt: time(-1) })).toBe(snapshot);
    expect(applyDownloadProgress(snapshot, { ...record, receivedBytes: 50 })).toBe(snapshot);
  });
  it('keeps cancellation against late progress and accepts a newer retry with reset bytes', () => {
    const { snapshot, record, index } = fixture();
    const cancelled: DownloadRecord = { ...record, status: 'cancelled', error: '下载已取消，可重试', updatedAt: time(400) };
    const stopped = applyDownloadProgress(snapshot, cancelled)!;
    expect(stopped.downloads[index]).toBe(cancelled);
    expect(applyDownloadProgress(stopped, { ...record, receivedBytes: 80, updatedAt: time(200) })).toBe(stopped);
    const retry: DownloadRecord = { ...record, status: 'queued', receivedBytes: 0, error: null, updatedAt: time(800) };
    expect(applyDownloadProgress(stopped, retry)!.downloads[index]).toBe(retry);
  });
  it('requests resync for unknown or mismatched identities, but keeps unsubscribed download history usable', () => {
    const { snapshot, record, index } = fixture();
    expect(applyDownloadProgress(snapshot, { ...record, id: 'unknown-download', updatedAt: time(400) })).toBeNull();
    expect(applyDownloadProgress(snapshot, { ...record, projectId: 'unknown-project', updatedAt: time(400) })).toBeNull();
    const orphan = { ...snapshot, projects: [] };
    expect(applyDownloadProgress(orphan, { ...record, receivedBytes: 50, updatedAt: time(400) })!.downloads[index].receivedBytes).toBe(50);
  });
  it('keeps in-flight progress when a slower full read returns, while accepting its metadata', () => {
    const { snapshot, record, index } = fixture();
    const progress = { ...record, receivedBytes: 75, updatedAt: time(800) };
    const current = applyDownloadProgress(snapshot, progress)!;
    const incoming = { ...snapshot, settings: { ...snapshot.settings, notifications: false } };
    const merged = mergeSnapshotDownloads(current, incoming, snapshot);
    expect(merged.downloads[index]).toBe(progress);
    expect(merged.settings).toBe(incoming.settings);
    expect(merged.projects).toBe(incoming.projects);
    const tied = { ...incoming, downloads: incoming.downloads.map((item) => item.id === record.id ? { ...item, updatedAt: time(800) } : item) };
    expect(mergeSnapshotDownloads(current, tied, snapshot).downloads[index]).toBe(progress);
  });
  it('accepts authoritative same-tick retry metadata when no newer event arrived during the read', () => {
    const { snapshot, record, index } = fixture();
    const cancelled: DownloadRecord = { ...record, status: 'cancelled', updatedAt: time(400) };
    const current = applyDownloadProgress(snapshot, cancelled)!;
    const retry: DownloadRecord = { ...cancelled, status: 'queued', receivedBytes: 0 };
    expect(applyDownloadProgress(current, retry)).toBe(current);
    const incoming = { ...current, downloads: current.downloads.map((item) => item.id === retry.id ? retry : item) };
    expect(mergeSnapshotDownloads(current, incoming, current).downloads[index]).toBe(retry);
  });
  it('accepts newer completion metadata and never re-adds records absent from a full read', () => {
    const { snapshot, record } = fixture();
    const current = applyDownloadProgress(snapshot, { ...record, receivedBytes: 50, updatedAt: time(400) })!;
    const incoming = { ...snapshot, downloads: snapshot.downloads.map((item) => item.id === record.id ? { ...item, status: 'completed' as const, receivedBytes: 100, updatedAt: time(800) } : item) };
    expect(mergeSnapshotDownloads(current, incoming, snapshot)).toBe(incoming);
    const removed = { ...incoming, downloads: [] };
    expect(mergeSnapshotDownloads(current, removed, snapshot)).toBe(removed);
    expect(mergeSnapshotDownloads(null, incoming, null)).toBe(incoming);
  });
});

describe('readable list states', () => {
  it('searches all words across names and repository identity', () => {
    const project = createPreviewSnapshot().projects[0];
    expect(matchesProject(project, 'lake1059 ffmpeg')).toBe(true);
    expect(matchesProject(project, 'kazumi')).toBe(false);
    expect(matchesProject(project, '  ')).toBe(true);
  });
  it('does not invent progress when the total is unknown', () => {
    expect(downloadPercent({ receivedBytes: 20, totalBytes: 0 })).toBe(0);
    expect(downloadPercent({ receivedBytes: 30, totalBytes: 100 })).toBe(30);
    expect(downloadPercent({ receivedBytes: 110, totalBytes: 100 })).toBe(100);
    expect(downloadPercent({ receivedBytes: Number.NaN, totalBytes: 100 })).toBe(0);
    expect(downloadPercent({ receivedBytes: -20, totalBytes: 100 })).toBe(0);
  });
  it('formats sizes without NaN output', () => {
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(Number.NaN)).toBe('大小未知');
  });
  it('never prints a credential-like value or serializes arbitrary error objects', () => {
    expect(readableError(new Error(`failed: ${'ghp_' + 'x'.repeat(30)}`))).toBe('failed: 〔已隐藏的凭据〕');
    expect(readableError({ token: 'not-a-real-token' })).toBe('操作未完成，请稍后重试。');
  });
  it('keeps secrets absent and marks development fixtures as plugin data', () => {
    const preview = createPreviewSnapshot();
    expect(preview.runtime.plugin).toBe(true);

    expect(preview.runtime.githubTokenConfigured).toBe(false);
    expect(preview.runtime).not.toHaveProperty('token');
  });
});
