import type { InstallerToolPreference, ToolUpdateProgress, ToolUpdateResult } from "../types/domain";

export const TOOL_UPDATE_PREFERENCE_KEY = "bitig.toolUpdates.auto";
export type ToolUpdatePreference = "undecided" | "enabled" | "disabled";
type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;

export function readToolUpdatePreference(getStorage: () => PreferenceStorage): { preference: ToolUpdatePreference; error: string | null } {
  try {
    const saved = getStorage().getItem(TOOL_UPDATE_PREFERENCE_KEY);
    if (saved === null || saved === "undecided") return { preference: "undecided", error: null };
    if (saved === "enabled" || saved === "disabled") return { preference: saved, error: null };
    return { preference: "disabled", error: "The saved update preference is invalid. Automatic updates are off." };
  } catch {
    return { preference: "disabled", error: "The update preference could not be read. Automatic updates are off." };
  }
}

export function saveToolUpdatePreference(preference: ToolUpdatePreference, getStorage: () => PreferenceStorage): string | null {
  try {
    const storage = getStorage();
    storage.setItem(TOOL_UPDATE_PREFERENCE_KEY, preference);
    if (storage.getItem(TOOL_UPDATE_PREFERENCE_KEY) === preference) return null;
  } catch { /* A successful write must be confirmed before enabling automatic updates. */ }
  return "Your choice could not be saved. Automatic updates are off for this session. Please try again.";
}

export async function initializeToolUpdatePreference(
  getStorage: () => PreferenceStorage,
  readInstaller: () => Promise<InstallerToolPreference | null>,
  isCurrent: () => boolean,
): Promise<ReturnType<typeof readToolUpdatePreference> | null> {
  let current = readToolUpdatePreference(getStorage);
  if (current.preference !== "undecided" || current.error) return current;
  let installer: InstallerToolPreference | null | undefined;
  try { installer = await readInstaller(); } catch { /* Invalid or unreadable installer data keeps updates off. */ }
  if (!isCurrent()) return null;
  current = readToolUpdatePreference(getStorage);
  if (current.preference !== "undecided" || current.error || installer === null) return current;
  if (installer !== "enabled" && installer !== "disabled") {
    return { preference: "disabled", error: "The installer update preference could not be read. Automatic updates are off." };
  }
  const error = saveToolUpdatePreference(installer, getStorage);
  return { preference: error ? "disabled" : installer, error };
}

export interface ToolUpdateView {
  installing: boolean;
  refreshingStatus: boolean;
  progress: ToolUpdateProgress | null;
  restartRequired: boolean;
  notice: string | null;
  error: string | null;
}

export const initialToolUpdateView: ToolUpdateView = {
  installing: false, refreshingStatus: false, progress: null,
  restartRequired: false, notice: null, error: null,
};

export interface ToolUpdateSession {
  autoAttempted: boolean;
  operationId: string | null;
  mounted: boolean;
  listener: { ready: Promise<boolean>; dispose: () => void } | null;
}

export function createToolUpdateSession(): ToolUpdateSession {
  return { autoAttempted: false, operationId: null, mounted: false, listener: null };
}

export const TOOL_UPDATE_RESTART_NOTICE = "Media tools have been updated. Restart BITIG to use the new versions.";

export function isToolUpdateConflict(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "conflict";
}

export function toolUpdateProgressLabel(progress: ToolUpdateProgress): string {
  switch (progress.stage) {
    case "checking": return "Checking official releases…";
    case "downloading": return `Downloading ${progress.toolName ?? "tools"}…`;
    case "verifying": return `Verifying ${progress.toolName ?? "downloaded tools"}…`;
    case "installing": return "Installing verified tools…";
  }
}

export function connectToolProgress(
  session: ToolUpdateSession,
  subscribe: (receive: (progress: ToolUpdateProgress) => void) => Promise<() => void>,
  onProgress: (progress: ToolUpdateProgress) => void,
): () => void {
  session.listener?.dispose();
  session.mounted = true;
  let active = true;
  let stop: (() => void) | null = null;
  const ready = Promise.resolve().then(() => subscribe((progress) => {
    if (active && session.listener?.ready === ready && session.operationId === progress.operationId) onProgress(progress);
  })).then((unlisten) => {
    if (!active) { unlisten(); return false; }
    stop = unlisten;
    return true;
  }, () => false);
  const dispose = () => {
    if (!active) return;
    active = false;
    stop?.();
    stop = null;
    if (session.listener?.ready === ready) {
      session.mounted = false;
      session.listener = null;
    }
  };
  session.listener = { ready, dispose };
  return dispose;
}

interface ToolUpdateActions {
  preference: () => ToolUpdatePreference;
  canStart: () => boolean;
  createId: () => string;
  install: (operationId: string) => Promise<ToolUpdateResult>;
  refreshReport: () => Promise<void>;
  setView: (update: (previous: ToolUpdateView) => ToolUpdateView) => void;
  errorMessage: (error: unknown) => string;
}

export async function runToolUpdate(session: ToolUpdateSession, source: "automatic" | "manual", actions: ToolUpdateActions): Promise<boolean> {
  const automatic = source === "automatic";
  const eligible = () => !session.operationId && actions.canStart()
    && (!automatic || (!session.autoAttempted && actions.preference() === "enabled"));
  const listener = session.listener;
  if (!session.mounted || !listener || !eligible()) return false;
  const ready = await listener.ready;
  if (!session.mounted || session.listener !== listener || !eligible()) return false;
  if (!ready) {
    actions.setView((previous) => ({ ...previous, error: "Tool update progress could not be connected. Try installing again in About & Tools." }));
    return false;
  }
  let operationId: string;
  try { operationId = actions.createId(); }
  catch {
    actions.setView((previous) => ({ ...previous, error: "The tool update could not be started. Please try again." }));
    return false;
  }
  session.operationId = operationId;
  actions.setView((previous) => ({
    ...previous, installing: true, error: null, notice: previous.restartRequired ? previous.notice : null,
    progress: { operationId, stage: "checking", toolName: null, downloadedBytes: 0, totalBytes: 0 },
  }));
  if (automatic) session.autoAttempted = true;
  try {
    const result = await actions.install(operationId);
    if (session.mounted && session.operationId === operationId) {
      actions.setView((previous) => ({
        ...previous, restartRequired: previous.restartRequired || result.restartRequired,
        notice: previous.restartRequired || result.restartRequired ? TOOL_UPDATE_RESTART_NOTICE : null,
        refreshingStatus: true,
      }));
      try { await actions.refreshReport(); }
      catch {
        if (session.mounted) actions.setView((previous) => ({ ...previous, error: "Tools were checked, but their status could not be refreshed." }));
      }
    }
  } catch (error) {
    if (session.mounted && session.operationId === operationId && !(automatic && isToolUpdateConflict(error))) {
      actions.setView((previous) => ({ ...previous, error: actions.errorMessage(error) }));
    }
  } finally {
    if (session.operationId === operationId) {
      session.operationId = null;
      if (session.mounted) actions.setView((previous) => ({ ...previous, installing: false, refreshingStatus: false, progress: null }));
    }
  }
  return true;
}
