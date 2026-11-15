import type { GithubRepository, GithubStatus, Project } from './types';

export const GITHUB_DEVICE_URL = 'https://github.com/login/device';

/** A new account invalidates both pending requests and all previously visible private data. */
export function githubAccountKey(status: GithubStatus | null): string {
  return status?.connected && status.account ? status.account.login.toLowerCase() : '';
}

export function githubAvatarUrl(value: string | null | undefined): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === 'https:' && url.hostname === 'avatars.githubusercontent.com' && !url.username && !url.password && !url.port ? url.href : null;
  } catch { return null; }
}

export function filterGithubPage(items: GithubRepository[], query: string): GithubRepository[] {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return items.filter((repository) => {
    const text = `${repository.fullName} ${repository.description ?? ''} ${repository.language ?? ''} ${repository.topics.join(' ')}`.toLocaleLowerCase();
    return terms.every((term) => text.includes(term));
  });
}

export function isSubscribed(repository: string, projects: Pick<Project, 'owner' | 'repo'>[]): boolean {
  const name = repository.toLowerCase();
  return projects.some((project) => `${project.owner}/${project.repo}`.toLowerCase() === name);
}

export function repositoryEditError(description: string, homepage: string): string | null {
  if (description.length > 350) return '仓库说明最多 350 个字符。';
  if (!homepage.trim()) return null;
  if (homepage.length > 2000) return '主页地址过长，请缩短后重试。';
  try {
    const url = new URL(homepage.trim());
    if (url.protocol === 'https:' && !url.username && !url.password) return null;
  } catch { /* Explain the same validation rule for malformed URLs. */ }
  return '主页请填写完整的 HTTPS 地址，或留空移除。';
}
