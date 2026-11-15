export type Channel = 'stable' | 'preview';
export type AssetPreference = 'auto' | 'portable' | 'installer';
export type Theme = 'system' | 'light' | 'dark';
export type ProjectStatus = 'notChecked' | 'checking' | 'upToDate' | 'updateAvailable' | 'versionUnknown' | 'noRelease' | 'noCompatibleAsset' | 'error';
export type AssetKind = 'installer' | 'portable' | 'other';

export interface ReleaseAsset {
  id: number;
  name: string;
  size: number;
  url: string;
  contentType: string;
  digest: string | null;
  kind: AssetKind;
  compatible: boolean;
  score: number;
  reason: string;
}

export interface Release {
  id: number;
  tag: string;
  name: string;
  body: string;
  publishedAt: string;
  url: string;
  prerelease: boolean;
  assets: ReleaseAsset[];
}

export interface Project {
  id: string;
  owner: string;
  repo: string;
  name: string;
  description: string;
  url: string;
  avatarUrl: string | null;
  channel: Channel;
  preference: AssetPreference;
  assetPattern: string | null;
  installedVersion: string | null;
  installedPath: string | null;
  installedSource: 'manual' | 'managed' | null;
  release: Release | null;
  selectedAssetId: number | null;
  status: ProjectStatus;
  checkedAt: string | null;
  error: string | null;
  defaultBranch: string;
  sourceHeadSha: string | null;
  sourceAhead: boolean | null;
  createdAt: string;
}

export type DownloadStatus = 'queued' | 'downloading' | 'completed' | 'failed' | 'cancelled' | 'installing' | 'installed';
export interface DownloadRecord {
  id: string;
  projectId: string;
  projectName: string;
  assetId: number;
  fileName: string;
  tag: string;
  url: string;
  kind: AssetKind;
  totalBytes: number;
  receivedBytes: number;
  status: DownloadStatus;
  path: string | null;
  sha256: string | null;
  verified: boolean;
  error: string | null;
  createdAt: string;
  updatedAt: string;
}

export type BuildTaskStatus = 'queued' | 'claimed' | 'running' | 'succeeded' | 'failed' | 'cancelled';
export interface Settings {
  theme: Theme;
  checkIntervalMinutes: number;
  notifications: boolean;
  autoDownload: boolean;
  maxConcurrentDownloads: number;
}

export interface RuntimeInfo {
  appVersion: string;
  dataDir: string;
  portable: boolean;
  githubTokenConfigured: boolean;
  plugin: boolean;
  platform: string;
  arch: string;
  credentialError?: string;

}

export interface Snapshot {
  projects: Project[];
  downloads: DownloadRecord[];
  settings: Settings;
  runtime: RuntimeInfo;
}

export interface AddProjectInput {
  repository: string;
  channel: Channel;
  preference: AssetPreference;
  installedVersion?: string;
  installedPath?: string;
}

export interface ProjectChanges {
  channel?: Channel;
  preference?: AssetPreference;
  assetPattern?: string | null;
  installedVersion?: string | null;
  installedPath?: string | null;
  selectedAssetId?: number | null;
}

export interface InstallResult {
  kind: 'installer' | 'portable';
  message: string;
  path: string;
  installed: boolean;
}

export interface CheckResult {
  checked: number;
  updates: number;
  errors: number;
}

export interface ManagedVersion {
  downloadId: string;
  version: string;
  path: string;
  active: boolean;
}

export interface GithubAccount {
  login: string;
  name: string | null;
  avatarUrl: string | null;
  url: string;
}

export interface GithubLogin {
  state: 'waiting' | 'complete' | 'error';
  userCode: string | null;
  verificationUri: string;
  message: string | null;
}

export interface GithubStatus {
  available: boolean;
  connected: boolean;
  account: GithubAccount | null;
  login: GithubLogin | null;
  error: string | null;
  provider: 'github-cli' | 'token';
  tokenSupported?: boolean;
}

export type GithubRepositoryTab = 'explore' | 'starred' | 'owned';

export interface GithubRepository {
  id: number;
  name: string;
  fullName: string;
  owner: string;
  description: string | null;
  url: string;
  private: boolean;
  archived: boolean;
  fork: boolean;
  language: string | null;
  stars: number;
  forks: number;
  openIssues: number;
  updatedAt: string;
  defaultBranch: string;
  homepage: string | null;
  topics: string[];
  canAdmin: boolean;
  starred: boolean | null;
}

export interface GithubRepositoryPage {
  items: GithubRepository[];
  page: number;
  hasMore: boolean;
  totalCount: number | null;
}

export interface GithubRepositoryDetail {
  repository: GithubRepository;
  readme: string;
}

export interface GithubActivity {
  id: string;
  actor: string;
  avatarUrl: string | null;
  repository: string;
  type: string;
  summary: string;
  url: string;
  createdAt: string;
}

export interface GithubActivityPage {
  items: GithubActivity[];
  page: number;
  hasMore: boolean;
}
