import { useEffect, useRef, useState, type FormEvent, type PointerEvent as ReactPointerEvent } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { displayError } from "../services/backend";
import { palettes, type PaletteId, type ThemeMode } from "../theme";
import type { Folder } from "../types/domain";

interface SidebarProps {
  folders: Folder[];
  selectedFolderId: string | null;
  onSelect: (id: string | null) => void;
  search: string;
  onSearchChange: (value: string) => void;
  onSearchSubmit: (event: FormEvent<HTMLFormElement>) => void;
  folderCreatorOpen: boolean;
  folderName: string;
  folderError: string | null;
  onNewFolder: () => void;
  onCloseFolderCreator: () => void;
  onFolderNameChange: (name: string) => void;
  onSubmitFolder: (event: FormEvent<HTMLFormElement>) => void;
  onRenameFolder: (id: string, name: string) => Promise<void>;
  onReorderFolder: (id: string, targetId: string, placement: "before" | "after") => Promise<void>;
  onDeleteFolder: (folder: Folder) => void;
  activeDownloads: number;
  onOpenDownloads: () => void;
  onOpenTools: () => void;
  theme: ThemeMode;
  onThemeChange: (theme: ThemeMode) => void;
  palette: PaletteId;
  onPaletteChange: (palette: PaletteId) => void;
}

interface FolderRowProps {
  folder: Folder;
  allFolders: Folder[];
  depth: number;
  selectedFolderId: string | null;
  onSelect: (id: string) => void;
  onRename: (id: string, name: string) => Promise<void>;
  onDelete: (folder: Folder) => void;
  draggedFolderId: string | null;
  dropTarget: { id: string; placement: "before" | "after" } | null;
  onKeyboardReorder: (folder: Folder, direction: "up" | "down") => void;
}

function FolderRow({
  folder, allFolders, depth, selectedFolderId, onSelect, onRename, onDelete,
  draggedFolderId, dropTarget, onKeyboardReorder,
}: FolderRowProps) {
  const children = allFolders.filter((candidate) => candidate.parentId === folder.id);
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [newName, setNewName] = useState(folder.name);
  const [renameError, setRenameError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuTriggerRef = useRef<HTMLButtonElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);

  function focusMenuTrigger() {
    window.requestAnimationFrame(() => menuTriggerRef.current?.focus());
  }

  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuOpen(false);
        menuTriggerRef.current?.focus();
      }
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [menuOpen]);

  useEffect(() => {
    if (renaming) renameInputRef.current?.select();
  }, [renaming]);

  function beginRename() {
    setMenuOpen(false);
    setNewName(folder.name);
    setRenameError(null);
    setRenaming(true);
  }

  function cancelRename() {
    setRenaming(false);
    setRenameError(null);
    focusMenuTrigger();
  }

  async function submitRename(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const name = newName.trim();
    if (saving) return;
    if (!name) {
      setRenameError("Enter a folder name.");
      return;
    }
    if (name === folder.name) {
      cancelRename();
      return;
    }
    setSaving(true);
    setRenameError(null);
    try {
      await onRename(folder.id, name);
      setRenaming(false);
      focusMenuTrigger();
    } catch (error) {
      setRenameError(displayError(error, "Folder could not be renamed."));
      window.requestAnimationFrame(() => renameInputRef.current?.focus());
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <div
        className={`folder-row ${draggedFolderId === folder.id ? "is-dragging" : ""} ${dropTarget?.id === folder.id ? `is-drop-${dropTarget.placement}` : ""}`}
        data-folder-id={folder.id}
        ref={menuRef}
      >
        {renaming ? (
          <form className="folder-rename-form" style={{ paddingLeft: `${Math.min(18 + depth * 16, 36)}px` }} onSubmit={(event) => void submitRename(event)} onKeyDown={(event) => { if (event.key === "Escape" && !saving) cancelRename(); }} aria-busy={saving} autoComplete="off" noValidate>
            <label className="sr-only" htmlFor={`rename-folder-${folder.id}`}>New name for {folder.name}</label>
            <input ref={renameInputRef} id={`rename-folder-${folder.id}`} value={newName} onChange={(event) => { setNewName(event.target.value); setRenameError(null); }} maxLength={120} autoComplete="off" disabled={saving} aria-invalid={Boolean(renameError)} aria-describedby={renameError ? `rename-error-${folder.id}` : undefined} />
            <button type="submit" disabled={!newName.trim() || saving} aria-label={saving ? "Saving folder name" : `Save name for ${folder.name}`}>{saving ? "…" : "✓"}</button>
            <button type="button" onClick={cancelRename} disabled={saving} aria-label={`Cancel renaming ${folder.name}`}>×</button>
          </form>
        ) : (
          <>
            <button className={`nav-item folder-item ${selectedFolderId === folder.id ? "is-active" : ""}`} style={{ paddingLeft: `${18 + depth * 16}px` }} type="button" onClick={() => onSelect(folder.id)} aria-label={`${folder.name}, ${folder.hasContents ? "contains items" : "empty folder"}`} aria-keyshortcuts="ArrowUp ArrowDown" onKeyDown={(event) => {
              if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                event.preventDefault();
                onKeyboardReorder(folder, event.key === "ArrowUp" ? "up" : "down");
              }
            }}>
              <span className={`folder-glyph ${folder.hasContents ? "is-filled" : ""}`} aria-hidden="true" />
              <span className="nav-text">{folder.name}</span>
            </button>
            <button ref={menuTriggerRef} className="folder-menu-trigger" type="button" onClick={() => setMenuOpen((current) => !current)} aria-label={`Folder actions for ${folder.name}`} aria-expanded={menuOpen} />
            {menuOpen && <div className="folder-action-menu">
              <button className="folder-rename-action" type="button" onClick={beginRename}>RENAME FOLDER</button>
              <button type="button" onClick={() => { setMenuOpen(false); onDelete(folder); }}>DELETE FOLDER</button>
            </div>}
          </>
        )}
      </div>
      {renameError && <p className="sidebar-folder-error" id={`rename-error-${folder.id}`} role="alert">{renameError}</p>}
      {children.map((child) => <FolderRow key={child.id} folder={child} allFolders={allFolders} depth={depth + 1} selectedFolderId={selectedFolderId} onSelect={onSelect} onRename={onRename} onDelete={onDelete} draggedFolderId={draggedFolderId} dropTarget={dropTarget} onKeyboardReorder={onKeyboardReorder} />)}
    </>
  );
}

export function Sidebar({
  folders, selectedFolderId, onSelect, search, onSearchChange,
  onSearchSubmit, folderCreatorOpen, folderName, folderError, onNewFolder,
  onCloseFolderCreator, onFolderNameChange, onSubmitFolder, onRenameFolder, onReorderFolder, onDeleteFolder, activeDownloads, onOpenDownloads,
  onOpenTools, theme, onThemeChange, palette, onPaletteChange,
}: SidebarProps) {
  const roots = folders.filter((folder) => folder.parentId === null);
  const creatorRef = useRef<HTMLDivElement>(null);
  const folderInputRef = useRef<HTMLInputElement>(null);
  const settingsRef = useRef<HTMLDivElement>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [appVersion, setAppVersion] = useState("0.1.0");
  const [draggedFolderId, setDraggedFolderId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<{ id: string; placement: "before" | "after" } | null>(null);
  const [reordering, setReordering] = useState(false);
  const [reorderError, setReorderError] = useState<string | null>(null);
  const dragCleanupRef = useRef<(() => void) | null>(null);
  const suppressClickRef = useRef(false);

  function clearDrag() {
    setDraggedFolderId(null);
    setDropTarget(null);
  }

  async function reorder(id: string, targetId: string, placement: "before" | "after") {
    if (reordering || id === targetId) return;
    setReordering(true);
    setReorderError(null);
    try {
      await onReorderFolder(id, targetId, placement);
    } catch (error) {
      setReorderError(displayError(error, "Folder order could not be saved."));
    } finally {
      setReordering(false);
    }
  }

  function canDrop(id: string, target: Folder) {
    const dragged = folders.find((folder) => folder.id === id);
    return dragged !== undefined && dragged.id !== target.id && dragged.parentId === target.parentId;
  }

  function handleFolderPointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || event.pointerType === "touch" || reordering || dragCleanupRef.current) return;
    const origin = event.target;
    if (!(origin instanceof Element) || origin.closest(".folder-action-menu, .folder-rename-form")) return;
    const sourceRow = origin.closest<HTMLElement>(".folder-row[data-folder-id]");
    const sourceIdCandidate = sourceRow?.dataset.folderId;
    if (!sourceIdCandidate || !folders.some((folder) => folder.id === sourceIdCandidate)) return;
    const sourceId: string = sourceIdCandidate;

    const startX = event.clientX;
    const startY = event.clientY;
    const pointerId = event.pointerId;
    const previousUserSelect = document.body.style.userSelect;
    let active = false;

    function targetAt(x: number, y: number) {
      const hovered = document.elementFromPoint(x, y);
      const row = hovered?.closest<HTMLElement>(".folder-row[data-folder-id]");
      const target = folders.find((folder) => folder.id === row?.dataset.folderId);
      if (!row || !target || !canDrop(sourceId, target)) return null;
      const bounds = row.getBoundingClientRect();
      return { id: target.id, placement: y < bounds.top + bounds.height / 2 ? "before" as const : "after" as const };
    }

    function move(pointer: PointerEvent) {
      if (pointer.pointerId !== pointerId) return;
      if (!active) {
        if (Math.hypot(pointer.clientX - startX, pointer.clientY - startY) < 6) return;
        active = true;
        document.body.style.userSelect = "none";
        setDraggedFolderId(sourceId);
        setReorderError(null);
      }
      pointer.preventDefault();
      const target = targetAt(pointer.clientX, pointer.clientY);
      setDropTarget((current) => current?.id === target?.id && current?.placement === target?.placement
        ? current : target);
    }

    function cleanup() {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("blur", cancel);
      if (active) document.body.style.userSelect = previousUserSelect;
      dragCleanupRef.current = null;
      clearDrag();
    }

    function cancel() {
      cleanup();
    }

    function finish(pointer: PointerEvent) {
      if (pointer.pointerId !== pointerId) return;
      const target = active ? targetAt(pointer.clientX, pointer.clientY) : null;
      if (active) {
        suppressClickRef.current = true;
        window.setTimeout(() => { suppressClickRef.current = false; }, 0);
      }
      cleanup();
      if (target) void reorder(sourceId, target.id, target.placement);
    }

    dragCleanupRef.current = cleanup;
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("blur", cancel);
  }

  useEffect(() => () => dragCleanupRef.current?.(), []);

  function handleKeyboardReorder(folder: Folder, direction: "up" | "down") {
    const siblings = folders.filter((candidate) => candidate.parentId === folder.parentId);
    const index = siblings.findIndex((candidate) => candidate.id === folder.id);
    const neighbor = siblings[index + (direction === "up" ? -1 : 1)];
    if (neighbor) void reorder(folder.id, neighbor.id, direction === "up" ? "before" : "after");
  }

  useEffect(() => {
    let disposed = false;
    void getVersion().then((version) => {
      if (!disposed) setAppVersion(version);
    }).catch(() => undefined);
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    if (!folderCreatorOpen) return;
    folderInputRef.current?.focus();
    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!creatorRef.current?.contains(event.target as Node)) onCloseFolderCreator();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCloseFolderCreator();
    };
    document.addEventListener("pointerdown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [folderCreatorOpen, onCloseFolderCreator]);

  useEffect(() => {
    if (!settingsOpen) return;
    const close = (event: PointerEvent) => {
      if (!settingsRef.current?.contains(event.target as Node)) setSettingsOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSettingsOpen(false);
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [settingsOpen]);

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-name">BITIG</span>
      </div>

      <form className="sidebar-search" role="search" onSubmit={onSearchSubmit} autoComplete="off">
        <span aria-hidden="true" />
        <label className="sr-only" htmlFor="library-search">Search library</label>
        <input id="library-search" value={search} onChange={(event) => onSearchChange(event.target.value)} placeholder="Search your library..." autoComplete="off" />
      </form>

      <nav aria-label="Media library">
        <p className="nav-label">LIBRARY</p>
        <button className={`nav-item ${selectedFolderId === null ? "is-active" : ""}`} type="button" onClick={() => onSelect(null)}>
          <span className="grid-glyph" aria-hidden="true" /><span className="nav-text">All media</span>
        </button>

        <div className="folder-heading-wrap" ref={creatorRef}>
          <div className="nav-heading-row">
            <p className="nav-label">FOLDERS</p>
            <button className="icon-button" type="button" onClick={onNewFolder} aria-label="Create folder" aria-expanded={folderCreatorOpen} aria-controls="folder-creator" />
          </div>
          {folderCreatorOpen && (
            <div className="folder-popover" id="folder-creator" role="dialog" aria-labelledby="folder-creator-title">
              <div className="popover-heading"><div><p className="eyebrow">ORGANIZE</p><h2 id="folder-creator-title">New folder</h2></div><button className="popover-close" type="button" onClick={onCloseFolderCreator} aria-label="Close folder form">×</button></div>
              <form onSubmit={onSubmitFolder} autoComplete="off" noValidate>
                <label htmlFor="folder-name">Folder name</label>
                <input ref={folderInputRef} id="folder-name" value={folderName} onChange={(event) => onFolderNameChange(event.target.value)} maxLength={120} autoComplete="off" />
                {folderError && <p className="form-error" role="alert">{folderError}</p>}
                <button className="primary-button" type="submit" disabled={!folderName.trim()}>CREATE</button>
              </form>
            </div>
          )}
        </div>

        <div className="folder-tree" onPointerDown={handleFolderPointerDown} onDragStart={(event) => event.preventDefault()} onClickCapture={(event) => {
          if (!suppressClickRef.current) return;
          event.preventDefault();
          event.stopPropagation();
          suppressClickRef.current = false;
        }}>
          {roots.length ? roots.map((folder) => <FolderRow key={folder.id} folder={folder} allFolders={folders} depth={0} selectedFolderId={selectedFolderId} onSelect={onSelect} onRename={onRenameFolder} onDelete={onDeleteFolder} draggedFolderId={draggedFolderId} dropTarget={dropTarget} onKeyboardReorder={handleKeyboardReorder} />) : <p className="sidebar-empty">No folders yet</p>}
          {reorderError && <p className="sidebar-folder-error" role="alert">{reorderError}</p>}
          {folderError && !folderCreatorOpen && <p className="sidebar-folder-error" role="alert">{folderError}</p>}
        </div>
      </nav>

      <button className="tools-trigger" type="button" onClick={onOpenTools} aria-label="Open FFmpeg and yt-dlp tools">
        <span className="tools-glyph" aria-hidden="true">+</span>
        <span className="tool-trigger-copy"><strong>TOOLS</strong><span>FFmpeg &amp; yt-dlp</span></span>
        <span className="chevron">›</span>
      </button>

      <div className="sidebar-settings" ref={settingsRef}>
        <button className="settings-trigger" type="button" onClick={() => setSettingsOpen((current) => !current)} aria-expanded={settingsOpen} aria-label="Appearance settings">
          <span className="settings-glyph" aria-hidden="true">◆</span><span className="nav-text">APPEARANCE</span>
        </button>
        {settingsOpen && (
          <div className="settings-popover" role="dialog" aria-label="Appearance settings">
            <div className="popover-heading"><div><p className="eyebrow">APPEARANCE</p><h2>Make it yours</h2></div><button className="popover-close" type="button" onClick={() => setSettingsOpen(false)} aria-label="Close settings">×</button></div>
            <p className="settings-label">MODE</p>
            <div className="theme-options">
              <button type="button" className={theme === "light" ? "is-selected" : ""} onClick={() => onThemeChange("light")}>LIGHT</button>
              <button type="button" className={theme === "dark" ? "is-selected" : ""} onClick={() => onThemeChange("dark")}>DARK</button>
            </div>
            <p className="settings-label">COLOR PALETTE</p>
            <div className="palette-list">
              {palettes.map((entry) => (
                <button key={entry.id} type="button" className={`palette-option ${palette === entry.id ? "is-selected" : ""}`} onClick={() => onPaletteChange(entry.id)}>
                  <span className="palette-name">{entry.name}</span>
                  <span className="palette-swatches">{entry.colors.map((color) => <i key={color} style={{ background: color }} />)}</span>
                </button>
              ))}
            </div>
          </div>
        )}
      </div>

      <button className="download-summary" type="button" onClick={onOpenDownloads} aria-label={activeDownloads ? `Downloads, ${activeDownloads} active` : "Downloads, queue is clear"}>
        <span className="pulse-dot" />
        <span className="download-copy"><strong>DOWNLOADS</strong><span>{activeDownloads ? `${activeDownloads} active` : "Queue is clear"}</span></span>
        <span className="chevron">›</span>
      </button>
      <div className="sidebar-version" aria-label={`BITIG version ${appVersion}`}><span>BITIG</span><strong>v{appVersion}</strong></div>
    </aside>
  );
}
