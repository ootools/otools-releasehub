import type { DownloadRecord, Snapshot } from './types';

const statuses = ['queued', 'downloading', 'completed', 'failed', 'cancelled', 'installing', 'installed'];
const kinds = ['portable', 'installer', 'other'];
const textFields = ['id', 'projectId', 'projectName', 'fileName', 'tag', 'url', 'createdAt', 'updatedAt'];
const optionalTextFields = ['path', 'sha256', 'error'];
const numericFields = ['assetId', 'totalBytes', 'receivedBytes'];
const validTime = (value: unknown): boolean => typeof value === 'string' && Number.isFinite(Date.parse(value));

/** Event payloads are checked before they can replace a complete UI record. */
export function isDownloadRecord(value: unknown): value is DownloadRecord {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return textFields.every((key) => typeof record[key] === 'string')
    && record.id !== '' && record.projectId !== '' && record.fileName !== ''
    && optionalTextFields.every((key) => record[key] === null || typeof record[key] === 'string')
    && numericFields.every((key) => { const number = record[key]; return typeof number === 'number' && Number.isSafeInteger(number) && number >= 0; })
    && typeof record.verified === 'boolean'
    && typeof record.status === 'string' && statuses.includes(record.status)
    && typeof record.kind === 'string' && kinds.includes(record.kind)
    && validTime(record.createdAt) && validTime(record.updatedAt);
}

function sameDownload(left: DownloadRecord, right: DownloadRecord): boolean {
  return left.id === right.id && left.projectId === right.projectId && left.assetId === right.assetId && left.createdAt === right.createdAt;
}

/** null asks for one coalesced metadata refresh; old notifications are a no-op. */
export function applyDownloadProgress(snapshot: Snapshot, record: DownloadRecord): Snapshot | null {
  const index = snapshot.downloads.findIndex((item) => item.id === record.id);
  if (index < 0 || !sameDownload(snapshot.downloads[index], record)) return null;
  // Millisecond ties cannot prove a newer attempt. Metadata resolves same-tick cancel/retry transitions.
  if (Date.parse(record.updatedAt) <= Date.parse(snapshot.downloads[index].updatedAt)) return snapshot;
  const downloads = snapshot.downloads.slice();
  downloads[index] = record;
  return { ...snapshot, downloads };
}

/** Keep progress received during a slower snapshot read, without restoring removed records. */
export function mergeSnapshotDownloads(current: Snapshot | null, incoming: Snapshot, atRequestStart: Snapshot | null): Snapshot {
  if (!current) return incoming;
  const latest = new Map(current.downloads.map((record) => [record.id, record]));
  const initial = new Map(atRequestStart?.downloads.map((record) => [record.id, record]) ?? []);
  let keptProgress = false;
  const downloads = incoming.downloads.map((record) => {
    const progress = latest.get(record.id);
    if (!progress || !sameDownload(progress, record)) return record;
    const elapsed = Date.parse(progress.updatedAt) - Date.parse(record.updatedAt);
    if (elapsed > 0 || (elapsed === 0 && progress !== initial.get(record.id))) {
      keptProgress = true;
      return progress;
    }
    return record;
  });
  return keptProgress ? { ...incoming, downloads } : incoming;
}
