import { useCallback, useEffect, useState } from "react";
import { backend, displayError, type FolderDeleteMode } from "../services/backend";
import type { AppError, Folder, Media } from "../types/domain";

interface LibraryState {
  folders: Folder[];
  media: Media[];
  selectedFolderId: string | null;
  loading: boolean;
  error: AppError | null;
}

const initialState: LibraryState = {
  folders: [],
  media: [],
  selectedFolderId: null,
  loading: true,
  error: null,
};

function asAppError(error: unknown): AppError {
  return { code: "unexpected", message: displayError(error, "The library could not be loaded.") };
}

export function useLibrary() {
  const [state, setState] = useState(initialState);

  const load = useCallback(async (folderId: string | null) => {
    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const [folders, media] = await Promise.all([
        backend.listFolders(),
        backend.listMedia(folderId),
      ]);
      setState((current) => ({ ...current, folders, media, loading: false }));
    } catch (error) {
      setState((current) => ({ ...current, loading: false, error: asAppError(error) }));
    }
  }, []);

  useEffect(() => {
    void load(null);
  }, [load]);

  const selectFolder = useCallback(async (folderId: string | null) => {
    setState((current) => ({ ...current, selectedFolderId: folderId, loading: true, error: null }));
    try {
      const media = await backend.listMedia(folderId);
      setState((current) => ({ ...current, media, loading: false }));
    } catch (error) {
      setState((current) => ({ ...current, loading: false, error: asAppError(error) }));
    }
  }, []);

  const createFolder = useCallback(async (name: string, parentId: string | null) => {
    const folder = await backend.createFolder(name, parentId);
    setState((current) => ({
      ...current,
      folders: [...current.folders.map((item) => item.id === parentId ? { ...item, hasContents: true } : item), folder],
    }));
    return folder;
  }, []);

  const renameFolder = useCallback(async (id: string, name: string) => {
    const renamed = await backend.renameFolder(id, name);
    setState((current) => ({
      ...current,
      folders: current.folders.map((folder) => folder.id === id ? renamed : folder),
    }));
  }, []);

  const reorderFolder = useCallback(async (id: string, targetId: string, placement: "before" | "after") => {
    const folders = await backend.reorderFolder(id, targetId, placement);
    setState((current) => ({ ...current, folders }));
  }, []);

  const deleteFolder = useCallback(async (id: string, mode: FolderDeleteMode) => {
    await backend.deleteFolder(id, mode);
    let selectedFolderId = state.selectedFolderId;
    if (mode === "delete_contents") {
      let ancestorId = selectedFolderId;
      while (ancestorId && ancestorId !== id) {
        ancestorId = state.folders.find((folder) => folder.id === ancestorId)?.parentId ?? null;
      }
      if (ancestorId === id) selectedFolderId = null;
    } else if (selectedFolderId === id) {
      selectedFolderId = null;
    }
    const nextFolderId = selectedFolderId;
    if (nextFolderId !== state.selectedFolderId) {
      setState((current) => ({ ...current, selectedFolderId: nextFolderId }));
    }
    await load(nextFolderId);
  }, [load, state.folders, state.selectedFolderId]);

  const search = useCallback(async (query: string) => {
    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const media = query.trim()
        ? await backend.searchLibrary(query)
        : await backend.listMedia(state.selectedFolderId);
      setState((current) => ({ ...current, media, loading: false }));
    } catch (error) {
      setState((current) => ({ ...current, loading: false, error: asAppError(error) }));
    }
  }, [state.selectedFolderId]);

  const reload = useCallback(() => load(state.selectedFolderId), [load, state.selectedFolderId]);

  return { ...state, selectFolder, createFolder, renameFolder, reorderFolder, deleteFolder, search, reload };
}
