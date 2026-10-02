import { invoke } from "@tauri-apps/api/core";
import type { AppError, DownloadJob, Folder, Media, MediaAnalysis, ToolStatusReport, ToolUpdateResult } from "../types/domain";

export type FolderDeleteMode = "move_contents" | "delete_contents";

class BackendError extends Error implements AppError {
  constructor(public readonly code: string, message: string) {
    super(message);
    this.name = "BackendError";
  }
}

const errorMessages: Record<string, string> = {
  validation: "Check the entered information and try again.",
  invalid_url: "Enter a valid HTTP or HTTPS media link.",
  tool_unavailable: "Media tools are unavailable. Reinstall BITIG to restore them.",
  tool_integrity_failed: "A media tool could not be verified. Reinstall BITIG.",
  tool_update_failed: "Tools could not be updated. Check your connection and try again.",
  analysis_failed: "This link could not be analyzed. Check it and try again.",
  download_failed: "The download could not be completed. You can try again.",
  media_validation_failed: "The downloaded file could not be verified. Try again.",
  process_output_too_large: "This link returned too much information to process.",
  process_timeout: "The operation took too long. Try again.",
  cancelled: "The operation was cancelled.",
  not_found: "This item is no longer available. Refresh and try again.",
  conflict: "This item changed before the action could finish. Try again.",
  not_empty: "Move or delete the items inside this folder first.",
  unsafe_path: "Choose a different location for this file.",
  storage_error: "The file could not be updated. Check available space and try again.",
  database_error: "The library could not be updated. Try again.",
  unexpected: "Something went wrong. Please try again.",
};

const commandErrorMessages: Record<string, Record<string, string>> = {
  create_folder: {
    conflict: "A folder with this name already exists here.",
    validation: "Enter a folder name of up to 120 characters.",
  },
  rename_folder: {
    conflict: "A folder with this name already exists here.",
    validation: "Enter a folder name of up to 120 characters.",
  },
  delete_folder: { conflict: "Cancel downloads targeting this folder before deleting its contents." },
  reorder_folder: { validation: "Folders can only be reordered within the same folder." },
  rename_media: { validation: "Enter a name of up to 240 characters." },
  start_download: { validation: "Check the media name and selected folder, then try again." },
  search_library: { validation: "Keep the search under 200 characters." },
  export_media: { validation: "Choose a valid destination and matching file format." },
};

function normalizeError(error: unknown, command: string): BackendError {
  const code = typeof error === "object" && error !== null && "code" in error && typeof error.code === "string"
    ? error.code
    : "unexpected";
  return new BackendError(code, commandErrorMessages[command]?.[code] ?? errorMessages[code] ?? "Something went wrong. Please try again.");
}

export function displayError(error: unknown, fallback: string): string {
  return error instanceof BackendError ? error.message : fallback;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error, command);
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
  reorderFolder: (id: string, targetId: string, placement: "before" | "after") =>
    call<Folder[]>("reorder_folder", { input: { id, targetId, placement } }),
  deleteFolder: (id: string, mode: FolderDeleteMode) => call<void>("delete_folder", { id, mode }),
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
  installToolUpdates: (operationId: string) =>
    call<ToolUpdateResult>("install_tool_updates", { operationId }),
  restartApp: () => call<void>("restart_app"),
};
