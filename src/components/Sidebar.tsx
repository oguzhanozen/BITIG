import { useEffect, useRef, useState, type FormEvent } from "react";
import { getVersion } from "@tauri-apps/api/app";
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
  onDelete: (folder: Folder) => void;
}

function FolderRow({ folder, allFolders, depth, selectedFolderId, onSelect, onDelete }: FolderRowProps) {
  const children = allFolders.filter((candidate) => candidate.parentId === folder.id);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [menuOpen]);

  return (
    <>
      <div className="folder-row" ref={menuRef}>
        <button className={`nav-item folder-item ${selectedFolderId === folder.id ? "is-active" : ""}`} style={{ paddingLeft: `${18 + depth * 16}px` }} type="button" title={folder.name} onClick={() => onSelect(folder.id)}>
          <span className="folder-glyph" aria-hidden="true" />
          <span className="nav-text">{folder.name}</span>
        </button>
        <button className="folder-menu-trigger" type="button" onClick={() => setMenuOpen((current) => !current)} aria-label={`Folder actions for ${folder.name}`} aria-expanded={menuOpen}>⋮</button>
        {menuOpen && <div className="folder-action-menu"><button type="button" onClick={() => { setMenuOpen(false); onDelete(folder); }}>DELETE FOLDER</button></div>}
      </div>
      {children.map((child) => <FolderRow key={child.id} folder={child} allFolders={allFolders} depth={depth + 1} selectedFolderId={selectedFolderId} onSelect={onSelect} onDelete={onDelete} />)}
    </>
  );
}

export function Sidebar({
  folders, selectedFolderId, onSelect, search, onSearchChange,
  onSearchSubmit, folderCreatorOpen, folderName, folderError, onNewFolder,
  onCloseFolderCreator, onFolderNameChange, onSubmitFolder, onDeleteFolder, activeDownloads, onOpenDownloads,
  onOpenTools, theme, onThemeChange, palette, onPaletteChange,
}: SidebarProps) {
  const roots = folders.filter((folder) => folder.parentId === null);
  const creatorRef = useRef<HTMLDivElement>(null);
  const folderInputRef = useRef<HTMLInputElement>(null);
  const settingsRef = useRef<HTMLDivElement>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [appVersion, setAppVersion] = useState("0.1.0");

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

      <form className="sidebar-search" role="search" onSubmit={onSearchSubmit}>
        <span aria-hidden="true" />
        <label className="sr-only" htmlFor="library-search">Search library</label>
        <input id="library-search" value={search} onChange={(event) => onSearchChange(event.target.value)} placeholder="Search your library..." />
      </form>

      <nav aria-label="Media library">
        <p className="nav-label">LIBRARY</p>
        <button className={`nav-item ${selectedFolderId === null ? "is-active" : ""}`} type="button" title="All media" onClick={() => onSelect(null)}>
          <span className="grid-glyph" aria-hidden="true" /><span className="nav-text">All media</span>
        </button>

        <div className="folder-heading-wrap" ref={creatorRef}>
          <div className="nav-heading-row">
            <p className="nav-label">FOLDERS</p>
            <button className="icon-button" type="button" onClick={onNewFolder} aria-label="Create folder" aria-expanded={folderCreatorOpen} aria-controls="folder-creator">+</button>
          </div>
          {folderCreatorOpen && (
            <div className="folder-popover" id="folder-creator" role="dialog" aria-labelledby="folder-creator-title">
              <div className="popover-heading"><div><p className="eyebrow">ORGANIZE</p><h2 id="folder-creator-title">New folder</h2></div><button className="popover-close" type="button" onClick={onCloseFolderCreator} aria-label="Close folder form">×</button></div>
              <form onSubmit={onSubmitFolder}>
                <label htmlFor="folder-name">Folder name</label>
                <input ref={folderInputRef} id="folder-name" value={folderName} onChange={(event) => onFolderNameChange(event.target.value)} maxLength={120} />
                {folderError && <p className="form-error" role="alert">{folderError}</p>}
                <button className="primary-button" type="submit" disabled={!folderName.trim()}>CREATE</button>
              </form>
            </div>
          )}
        </div>

        <div className="folder-tree">
          {roots.length ? roots.map((folder) => <FolderRow key={folder.id} folder={folder} allFolders={folders} depth={0} selectedFolderId={selectedFolderId} onSelect={onSelect} onDelete={onDeleteFolder} />) : <p className="sidebar-empty">No folders yet</p>}
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
