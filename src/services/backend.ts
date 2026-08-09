import { invoke } from "@tauri-apps/api/core";
import type { AppError, DownloadJob, Folder, Media, MediaAnalysis, ToolStatusReport } from "../types/domain";

class BackendError extends Error implements AppError {
  constructor(public readonly code: string, message: string) {
    super(message);
    this.name = "BackendError";
  }
}

function normalizeError(error: unknown): BackendError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof error.code === "string" &&
    typeof error.message === "string"
  ) {
    return new BackendError(error.code, error.message);
  }
  return new BackendError(
    "unexpected",
    error instanceof Error ? error.message : "Something unexpected happened.",
  );
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

export const backend = {
  analyzeUrl: (url: string) => call<MediaAnalysis>("analyze_url", { url }),
  startDownload: (input: { analysisId: string; optionId: string; title: string; folderId: string | null }) =>
    call<DownloadJob>("start_download", { input }),
  listDownloads: () => call<DownloadJob[]>("list_downloads"),
  cancelDownload: (id: string) => call<DownloadJob>("cancel_download", { id }),
  listFolders: () => call<Folder[]>("list_folders"),
  createFolder: (name: string, parentId: string | null) =>
    call<Folder>("create_folder", { input: { name, parentId } }),
  renameFolder: (id: string, name: string) =>
    call<Folder>("rename_folder", { input: { id, name } }),
  deleteFolder: (id: string) => call<void>("delete_folder", { id }),
  listMedia: (folderId: string | null) => call<Media[]>("list_media", { folderId }),
  getMediaPlaybackPath: (id: string) => call<string>("get_media_playback_path", { id }),
  getMediaThumbnailPath: (id: string) =>
    call<string | null>("get_media_thumbnail_path", { id }),
  exportMedia: (id: string, destination: string) => call<void>("export_media", { id, destination }),
  searchLibrary: (query: string) => call<Media[]>("search_library", { query }),
  renameMedia: (id: string, name: string) =>
    call<Media>("rename_media", { input: { id, name } }),
  moveMedia: (id: string, folderId: string | null) =>
    call<Media>("move_media", { input: { id, folderId } }),
  deleteMedia: (id: string) => call<void>("delete_media", { id }),
  getToolStatus: (checkUpdates: boolean) =>
    call<ToolStatusReport>("get_tool_status", { checkUpdates }),
};
