export interface Folder {
  id: string;
  parentId: string | null;
  name: string;
  hasContents: boolean;
  createdAt: string;
  updatedAt: string;
}

export type MediaKind = "video" | "audio";

export interface Media {
  id: string;
  folderId: string | null;
  title: string;
  mediaType: MediaKind;
  sourceUrl: string;
  sourcePlatform: string;
  sourceId: string;
  creator: string | null;
  filePath: string;
  thumbnailPath: string | null;
  container: string;
  width: number | null;
  height: number | null;
  durationMs: number | null;
  fileSize: number;
  createdAt: string;
  updatedAt: string;
}

export interface AppError {
  code: string;
  message: string;
}

export interface DownloadOption {
  id: string;
  kind: MediaKind;
  resolution: number | null;
  fps: number | null;
  container: string;
  estimatedSize: number | null;
}

export interface MediaAnalysis {
  analysisId: string;
  title: string;
  sourcePlatform: string;
  sourceId: string;
  creator: string | null;
  thumbnailUrl: string | null;
  durationMs: number | null;
  existingVersions: number;
  options: DownloadOption[];
}

export type DownloadStatus = "queued" | "analyzing" | "downloading" | "processing" | "verifying" | "finalizing" | "completed" | "failed" | "cancelled";

export interface DownloadJob {
  id: string;
  mediaId: string | null;
  title: string;
  sourceUrl: string;
  status: DownloadStatus;
  progress: number;
  downloadedBytes: number | null;
  totalBytes: number | null;
  bytesPerSecond: number | null;
  errorCode: string | null;
  errorMessage: string | null;
  createdAt: string;
  startedAt: string | null;
  completedAt: string | null;
}

export type ToolUpdateState = "not_checked" | "current" | "update_available" | "check_failed";

export interface ToolVersionStatus {
  id: string;
  name: string;
  installedVersion: string;
  latestVersion: string | null;
  state: ToolUpdateState;
  integrityVerified: boolean;
  updateSupported: boolean;
  message: string | null;
}

export interface ToolStatusReport {
  checkedAt: string | null;
  tools: ToolVersionStatus[];
}

export interface ToolUpdateResult {
  updatedTools: string[];
  restartRequired: boolean;
}

export type ToolUpdateStage = "checking" | "downloading" | "verifying" | "installing";

export interface ToolUpdateProgress {
  operationId: string;
  stage: ToolUpdateStage;
  toolName: string | null;
  downloadedBytes: number;
  totalBytes: number;
}
