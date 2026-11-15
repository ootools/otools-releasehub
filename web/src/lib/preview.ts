import type { ManagedVersion, Project, Release, ReleaseAsset, Snapshot } from './types';

/** Example directories only: neither this fixture nor the preview creates files. */
export function createPreviewManagedVersions(projectId: string): ManagedVersion[] {
  if (projectId !== 'preview-ff') return [];
  return [
    { downloadId: 'preview-managed-64', version: '6.4.0', path: '.\\data\\apps\\preview-ff\\6.4.0', active: true },
    { downloadId: 'preview-managed-63', version: '6.3.0', path: '.\\data\\apps\\preview-ff\\6.3.0', active: false },
  ];
}

/** Imported only after an explicit development-only preview action. No real writes. */
export function createPreviewSnapshot(): Snapshot {
  const now = new Date().toISOString();
  const asset = (id: number, repository: string, tag: string, name: string, size: number, portable = false): ReleaseAsset => ({
    id, name, size, url: `https://github.com/${repository}/releases/download/${tag}/${name}`,
    contentType: 'application/octet-stream', digest: null, kind: portable ? 'portable' : 'installer',
    compatible: true, score: 95, reason: `Windows x64 · ${portable ? name.toLowerCase().endsWith('.exe') ? '单文件免安装程序' : '免安装压缩包' : '安装程序'}`,
  });
  const release = (id: number, repository: string, tag: string, assets: ReleaseAsset[], body: string): Release => ({
    id, tag, name: tag, body, publishedAt: now, url: `https://github.com/${repository}/releases`, prerelease: false, assets,
  });
  const project = (id: string, owner: string, repo: string, name: string, description: string, installedVersion: string, next: Release, update = true): Project => ({
    id, owner, repo, name, description, url: `https://github.com/${owner}/${repo}`, avatarUrl: null,
    channel: 'stable', preference: 'auto', assetPattern: null, installedVersion, installedPath: null,
    installedSource: 'manual', release: next, selectedAssetId: next.assets[0]?.id ?? null,
    status: update ? 'updateAvailable' : 'upToDate', checkedAt: now, error: null,
    defaultBranch: 'main', sourceHeadSha: null, sourceAhead: null, createdAt: now,
  });
  const ff = release(1, 'Lake1059/FFmpegFreeUI', '6.5.0', [
    asset(101, 'Lake1059/FFmpegFreeUI', '6.5.0', 'FFmpegFreeUI.x64.exe', 37_400_000, true),
    { ...asset(102, 'Lake1059/FFmpegFreeUI', '6.5.0', 'Updater.exe', 100_000), compatible: false, score: -50, reason: '更新辅助程序，不是主程序', kind: 'other' },
  ], '## 让创作更顺手\n\n> 这是界面示例，不是实时检查结果。版本和文件名参考公开发行，大小与状态仅作演示。\n\n- 查看清晰的发行动态\n- 核对适合 Windows x64 的文件\n- 阅读说明后，再决定是否更新\n\n### 安装包说明\nFFmpegFreeUI.x64.exe 是单文件免安装主程序，不是安装向导。Updater.exe 是辅助程序，不应当作主程序下载。\n\n[到 GitHub 阅读真实发行说明](https://github.com/Lake1059/FFmpegFreeUI/releases)');
  const apk = release(2, 'RoutineMiku/apkstart', 'v0.3.2', [
    asset(201, 'RoutineMiku/apkstart', 'v0.3.2', 'ApkStart-v0.3.2-win64-portable.zip', 119_537_664, true),
    asset(202, 'RoutineMiku/apkstart', 'v0.3.2', 'ApkStart-0.3.2-Setup.exe', 1_449_551_462),
    { ...asset(203, 'RoutineMiku/apkstart', 'v0.3.2', 'ApkStart-runtime-v0.3.2.zip', 1_428_076_626, true), compatible: false, score: -50, reason: '运行时组件包，不是主程序', kind: 'other' },
  ], '## 安装包选择示例\n\n此为界面预览，不是实时更新结果。\n\n- **portable.zip**：免安装主程序\n- **Setup.exe**：较大的安装程序\n- **runtime.zip**：运行时组件，不是独立主程序\n\n[查看原项目发行说明](https://github.com/RoutineMiku/apkstart/releases)');
  const kazumi = release(3, 'Predidit/Kazumi', '2.4.4', [
    asset(301, 'Predidit/Kazumi', '2.4.4', 'Kazumi_windows_2.4.4.zip', 42_991_616, true),
    { ...asset(302, 'Predidit/Kazumi', '2.4.4', 'Kazumi_windows_2.4.4.msix', 47_919_923), kind: 'other', compatible: false, reason: 'MSIX 请在原项目阅读安装要求；当前优先选择 ZIP' },
  ], '## 发行记录示例\n\n此为界面预览，已记录版本仅作演示。Windows 包的 x64 架构来自上游构建说明，而不是文件名猜测。\n\n[阅读真实发行说明](https://github.com/Predidit/Kazumi/releases)');
  return {
    projects: [
      { ...project('preview-ff', 'Lake1059', 'FFmpegFreeUI', 'FFmpegFreeUI', '把繁琐的音视频处理，变成直观的操作。', '6.4.0', ff), installedSource: 'managed', installedPath: createPreviewManagedVersions('preview-ff')[0].path },
      project('preview-apk', 'RoutineMiku', 'apkstart', 'ApkStart', '轻巧的 Android 应用包管理工具。', 'v0.3.1', apk),
      project('preview-kazumi', 'Predidit', 'Kazumi', 'Kazumi', '简洁、开放的跨平台番剧客户端。', '2.4.4', kazumi, false),
    ],
    downloads: [
      ...createPreviewManagedVersions('preview-ff').map((version) => ({ id: version.downloadId, projectId: 'preview-ff', projectName: 'FFmpegFreeUI', assetId: 100, fileName: ff.assets[0].name, tag: version.version, url: `https://github.com/Lake1059/FFmpegFreeUI/releases/download/${version.version}/${ff.assets[0].name}`, kind: 'portable' as const, totalBytes: ff.assets[0].size, receivedBytes: ff.assets[0].size, status: 'installed' as const, path: null, sha256: null, verified: false, error: null, createdAt: now, updatedAt: now })),
      { id: 'preview-dl1', projectId: 'preview-ff', projectName: 'FFmpegFreeUI', assetId: 101, fileName: ff.assets[0].name, tag: ff.tag, url: ff.assets[0].url, kind: 'portable', totalBytes: ff.assets[0].size, receivedBytes: ff.assets[0].size, status: 'completed', path: null, sha256: null, verified: false, error: null, createdAt: now, updatedAt: now },
      { id: 'preview-dl2', projectId: 'preview-apk', projectName: 'ApkStart', assetId: 201, fileName: apk.assets[0].name, tag: apk.tag, url: apk.assets[0].url, kind: 'portable', totalBytes: apk.assets[0].size, receivedBytes: 75_308_728, status: 'downloading', path: null, sha256: null, verified: false, error: null, createdAt: now, updatedAt: now },
    ],
    settings: { theme: 'system', checkIntervalMinutes: 60, notifications: true, autoDownload: false, maxConcurrentDownloads: 2, },
    runtime: { appVersion: '0.1.0', dataDir: '.\\data（界面示例）', portable: true, githubTokenConfigured: false, plugin:true,platform:'macos',arch:'aarch64' },
  };
}
