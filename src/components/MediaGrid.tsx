import { useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { Folder, Media } from "../types/domain";
import { formatDuration } from "../lib/format";
import { backend } from "../services/backend";

type CollectionView = "cards" | "compact" | "list";
type CollectionFilter = "all" | "video" | "audio" | "mp4" | "webm" | "mkv" | "m4a" | "mp3" | "wav";

const filters: { value: CollectionFilter; label: string }[] = [
  { value: "all", label: "All formats" },
  { value: "video", label: "All video" },
  { value: "audio", label: "All audio" },
  { value: "mp4", label: "MP4" },
  { value: "webm", label: "WEBM" },
  { value: "mkv", label: "MKV" },
  { value: "m4a", label: "M4A" },
  { value: "mp3", label: "MP3" },
  { value: "wav", label: "WAV" },
];

interface MediaGridProps {
  items: Media[];
  folders: Folder[];
  loading: boolean;
  folderName: string;
  onAddUrl: () => void;
  onSelect: (item: Media) => void;
  onChanged: () => void;
}

function isCollectionView(value: string | null): value is CollectionView {
  return value === "cards" || value === "compact" || value === "list";
}

function filterCollection(items: Media[], filter: CollectionFilter): Media[] {
  if (filter === "all") return items;
  if (filter === "video" || filter === "audio") {
    return items.filter((item) => item.mediaType === filter);
  }
  return items.filter((item) => item.container.toLowerCase() === filter);
}

export function MediaGrid({ items, folders, loading, folderName, onAddUrl, onSelect, onChanged }: MediaGridProps) {
  const [view, setView] = useState<CollectionView>(() => {
    const saved = localStorage.getItem("bitig.collectionView");
    return isCollectionView(saved) ? saved : "cards";
  });
  const [filter, setFilter] = useState<CollectionFilter>("all");
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [openMenuId, setOpenMenuId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const visibleItems = useMemo(() => filterCollection(items, filter), [filter, items]);
  const folderNames = useMemo(() => new Map(folders.map((folder) => [folder.id, folder.name])), [folders]);
  const selectedItems = items.filter((item) => selectedIds.includes(item.id));
  const allVisibleSelected = visibleItems.length > 0 && visibleItems.every((item) => selectedIds.includes(item.id));

  useEffect(() => {
    if (!openMenuId) return;
    const close = (event: PointerEvent) => {
      if (!(event.target as Element).closest("[data-card-menu]")) setOpenMenuId(null);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpenMenuId(null);
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [openMenuId]);

  function chooseView(next: CollectionView) {
    setView(next);
    localStorage.setItem("bitig.collectionView", next);
  }

  function toggleSelected(id: string) {
    setSelectedIds((current) => current.includes(id) ? current.filter((value) => value !== id) : [...current, id]);
  }

  function toggleAllVisible() {
    setSelectedIds((current) => {
      const visibleIds = visibleItems.map((item) => item.id);
      if (allVisibleSelected) return current.filter((id) => !visibleIds.includes(id));
      return Array.from(new Set([...current, ...visibleIds]));
    });
  }

  async function move(ids: string[], destination: string) {
    if (!ids.length || !destination) return;
    setBusy(true);
    setError(null);
    try {
      const folderId = destination === "__root__" ? null : destination;
      await Promise.all(ids.map((id) => backend.moveMedia(id, folderId)));
      setSelectedIds([]);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Media could not be moved.");
    } finally {
      onChanged();
      setBusy(false);
    }
  }

  async function remove(targets: Media[]) {
    if (!targets.length) return;
    const message = targets.length === 1
      ? `Delete “${targets[0]!.title}” from BITIG? This removes the managed file.`
      : `Delete ${targets.length} selected items from BITIG? This removes their managed files.`;
    if (!window.confirm(message)) return;
    setBusy(true);
    setError(null);
    try {
      await Promise.all(targets.map((item) => backend.deleteMedia(item.id)));
      setSelectedIds([]);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Media could not be deleted.");
    } finally {
      onChanged();
      setBusy(false);
    }
  }

  if (loading) {
    return <div className="empty-state"><div className="loading-ring" /><h2>Opening vault</h2></div>;
  }

  if (!items.length) {
    return (
      <div className="empty-state">
        <p className="eyebrow">YOUR COLLECTION STARTS HERE</p>
        <h2>Save it. Sort it. Keep it.</h2>
        <p>Bring in media you have permission to download and organize it in your own private library.</p>
        <button className="primary-button" type="button" onClick={onAddUrl}>+ ADD YOUR FIRST URL</button>
      </div>
    );
  }

  return (
    <section className="collection-section" aria-labelledby="collection-title">
      <div className="collection-heading">
        <div><p className="eyebrow">COLLECTION</p><h2 id="collection-title">{folderName}</h2></div>
        <div className="collection-controls">
          <label className="collection-filter">FORMAT
            <select value={filter} onChange={(event) => setFilter(event.target.value as CollectionFilter)}>
              {filters.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </select>
          </label>
          <div className="view-switcher" role="group" aria-label="Collection view">
            <button type="button" className={view === "cards" ? "is-selected" : ""} onClick={() => chooseView("cards")} aria-label="Card view" title="Card view">▦</button>
            <button type="button" className={view === "compact" ? "is-selected" : ""} onClick={() => chooseView("compact")} aria-label="Compact view" title="Compact view">▤</button>
            <button type="button" className={view === "list" ? "is-selected" : ""} onClick={() => chooseView("list")} aria-label="List view" title="List view">☷</button>
          </div>
          <span className="item-count">{visibleItems.length}/{items.length}</span>
        </div>
      </div>

      <div className={`selection-toolbar ${selectedItems.length ? "is-active" : ""}`}>
        <label className="select-all"><input type="checkbox" checked={allVisibleSelected} onChange={toggleAllVisible} /><span />{allVisibleSelected ? "CLEAR VISIBLE" : "SELECT ALL"}</label>
        {selectedItems.length > 0 && (
          <>
            <strong>{selectedItems.length} SELECTED</strong>
            <select aria-label="Move selected media" value="" disabled={busy} onChange={(event) => void move(selectedItems.map((item) => item.id), event.target.value)}>
              <option value="" disabled>MOVE TO…</option>
              <option value="__root__">All media</option>
              {folders.map((folder) => <option key={folder.id} value={folder.id}>{folder.name}</option>)}
            </select>
            <button className="bulk-delete" type="button" disabled={busy} onClick={() => void remove(selectedItems)}>DELETE</button>
            <button className="clear-selection" type="button" onClick={() => setSelectedIds([])}>CLEAR</button>
          </>
        )}
      </div>

      {error && <p className="collection-error" role="alert">{error}</p>}
      {!visibleItems.length ? (
        <div className="filtered-empty"><p>No media matches this format.</p><button type="button" onClick={() => setFilter("all")}>SHOW ALL FORMATS</button></div>
      ) : (
        <div className="media-collection" data-view={view}>
          {visibleItems.map((item) => (
            <article className={`media-card ${selectedIds.includes(item.id) ? "is-selected" : ""}`} key={item.id}>
              <label className="media-select" title={`Select ${item.title}`}>
                <input type="checkbox" checked={selectedIds.includes(item.id)} onChange={() => toggleSelected(item.id)} aria-label={`Select ${item.title}`} />
                <span />
              </label>
              <button className="media-card-open" type="button" onClick={() => onSelect(item)} aria-label={`Open ${item.title}`}>
                <MediaArtwork item={item} />
                <div className="media-card-copy">
                  <h3>{item.title}</h3>
                  <p className="media-folder"><i aria-hidden="true" />{item.folderId ? folderNames.get(item.folderId) ?? "Unknown folder" : "All media"}</p>
                  <span>{item.height ? `${item.height}p` : item.container.toUpperCase()}</span>
                </div>
                <div className="media-list-meta"><span>{item.mediaType}</span><span>{item.container.toUpperCase()}</span><span>{formatDuration(item.durationMs)}</span><span>{(item.fileSize / 1024 / 1024).toFixed(1)} MB</span></div>
              </button>
              <div className="card-menu" data-card-menu>
                <button className="card-menu-trigger" type="button" onClick={() => setOpenMenuId((current) => current === item.id ? null : item.id)} aria-label={`Actions for ${item.title}`} aria-expanded={openMenuId === item.id}>⋮</button>
                {openMenuId === item.id && <div className="card-action-menu">
                  <label>MOVE TO
                    <select aria-label={`Move ${item.title}`} value="" disabled={busy} onChange={(event) => { setOpenMenuId(null); void move([item.id], event.target.value); }}>
                      <option value="" disabled>Choose folder…</option>
                      <option value="__root__">All media</option>
                      {folders.map((folder) => <option key={folder.id} value={folder.id}>{folder.name}</option>)}
                    </select>
                  </label>
                  <button type="button" disabled={busy} onClick={() => { setOpenMenuId(null); void remove([item]); }}>DELETE FROM BITIG</button>
                </div>}
              </div>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

function MediaArtwork({ item }: { item: Media }) {
  const [thumbnail, setThumbnail] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    if (item.mediaType === "audio" || !item.thumbnailPath) return () => { current = false; };
    void backend.getMediaThumbnailPath(item.id)
      .then((path) => { if (current && path) setThumbnail(convertFileSrc(path)); })
      .catch(() => undefined);
    return () => { current = false; };
  }, [item.id, item.mediaType, item.thumbnailPath]);

  return (
    <div className={`media-art ${item.mediaType === "audio" ? "audio-art" : ""}`}>
      {item.mediaType === "audio" ? <span className="audio-clef" aria-hidden="true">𝄞</span> : thumbnail ? <img src={thumbnail} alt="" onError={() => setThumbnail(null)} /> : <span>▶</span>}
      <time>{formatDuration(item.durationMs)}</time>
    </div>
  );
}
