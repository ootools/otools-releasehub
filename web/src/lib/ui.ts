import type { DownloadRecord, DownloadStatus, ManagedVersion, Project, ProjectStatus } from './types';

export type Page = 'home' | 'library' | 'updates' | 'downloads' | 'github' | 'settings';
export type Tone = 'neutral' | 'success' | 'warning' | 'danger' | 'accent';
export interface StatusLabel { label: string; tone: Tone }

export const projectStatus: Record<ProjectStatus, StatusLabel> = {
  notChecked: { label: '待检查', tone: 'neutral' },
  checking: { label: '检查中', tone: 'accent' },
  upToDate: { label: '已是最新', tone: 'success' },
  updateAvailable: { label: '发现更新', tone: 'accent' },
  versionUnknown: { label: '未记录版本', tone: 'warning' },
  noRelease: { label: '暂无发行版', tone: 'neutral' },
  noCompatibleAsset: { label: '需选择安装包', tone: 'warning' },
  error: { label: '检查失败', tone: 'danger' },
};
export const downloadStatus: Record<DownloadStatus, StatusLabel> = {
  queued: { label: '等待下载', tone: 'neutral' },
  downloading: { label: '正在下载', tone: 'accent' },
  completed: { label: '已下载 · 待安装', tone: 'success' },
  failed: { label: '下载失败', tone: 'danger' },
  cancelled: { label: '已取消', tone: 'neutral' },
  installing: { label: '处理中', tone: 'accent' },
  installed: { label: '已安装', tone: 'success' },
};


export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '大小未知';
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let size = bytes / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit += 1; }
  return `${size >= 100 ? size.toFixed(0) : size.toFixed(1)} ${units[unit]}`;
}

export function formatDate(value: string | null, includeTime = false): string {
  if (!value || Number.isNaN(Date.parse(value))) return '尚未记录';
  return new Intl.DateTimeFormat('zh-CN', {
    month: 'short', day: 'numeric', ...(includeTime ? { hour: '2-digit', minute: '2-digit' } as const : {}),
  }).format(new Date(value));
}

export function downloadPercent(download: Pick<DownloadRecord, 'receivedBytes' | 'totalBytes'>): number {
  if (!Number.isFinite(download.totalBytes) || download.totalBytes <= 0 || !Number.isFinite(download.receivedBytes)) return 0;
  return Math.max(0, Math.min(100, Math.round(download.receivedBytes / download.totalBytes * 100)));
}

export function canActivateManagedVersion(project: Pick<Project, 'installedSource'> | null, version: ManagedVersion | undefined, confirmed: boolean): boolean {
  return Boolean(confirmed && project?.installedSource === 'managed' && version && !version.active);
}

export function matchesProject(project: Project, query: string): boolean {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const haystack = `${project.name} ${project.owner}/${project.repo} ${project.description}`.toLocaleLowerCase();
  return terms.every((term) => haystack.includes(term));
}

export function parseRepository(input: string): string | null {
  let value = input.trim();
  if (/^https?:\/\//i.test(value)) {
    const safe = safeGithubUrl(value);
    if (!safe) return null;
    const path = new URL(safe).pathname.split('/').filter(Boolean);
    if (path.length < 2 || (path.length > 2 && path[2] !== 'releases')) return null;
    value = `${path[0]}/${path[1]}`;
  }
  value = value.replace(/\/$/, '').replace(/\.git$/i, '');
  return /^[a-z\d](?:[a-z\d-]{0,38})\/[a-z\d_.-]{1,100}$/i.test(value) ? value : null;
}

/** Match the host's GitHub-only browser policy before forwarding a link. */
export function safeGithubUrl(value: string, base?: string): string | null {
  if (!value || value.trim().startsWith('#')) return null;
  try {
    const url = new URL(value, base);
    if (url.protocol !== 'https:' || url.hostname !== 'github.com' || url.username || url.password || url.port) return null;
    return url.href;
  } catch { return null; }
}

export function readableError(error: unknown): string {
  const message = typeof error === 'string' ? error : error instanceof Error ? error.message : '操作未完成，请稍后重试。';
  return message.replace(/(?:github_pat_|gh[pousr]_)[A-Za-z0-9_]+/g, '〔已隐藏的凭据〕').slice(0, 800);
}

export function projectInitials(project: Pick<Project, 'name' | 'repo'>): string {
  return (project.name || project.repo).replace(/[^A-Za-z\d\u4e00-\u9fff]/g, '').slice(0, 2).toUpperCase() || 'GH';
}
