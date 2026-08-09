import { useCallback, useEffect, useState } from "react";
import { backend } from "../services/backend";
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
  return typeof error === "object" && error !== null && "code" in error && "message" in error
    ? (error as AppError)
    : { code: "unexpected", message: "The library could not be loaded." };
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
    setState((current) => ({ ...current, folders: [...current.folders, folder] }));
    return folder;
  }, []);

  const deleteFolder = useCallback(async (id: string) => {
    await backend.deleteFolder(id);
    setState((current) => ({
      ...current,
      folders: current.folders.filter((folder) => folder.id !== id),
    }));
    if (state.selectedFolderId === id) await selectFolder(null);
  }, [selectFolder, state.selectedFolderId]);

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

  return { ...state, selectFolder, createFolder, deleteFolder, search, reload };
}
