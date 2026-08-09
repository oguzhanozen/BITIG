import { useEffect, useState, type FormEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { backend } from "../services/backend";
import type { Folder, Media } from "../types/domain";
import { formatDuration, sanitizeExportName } from "../lib/format";
import { SidePanel } from "./SidePanel";

interface Props {
  media: Media;
  folders: Folder[];
  onClose: () => void;
  onChanged: () => void;
}

export function MediaDetailDialog({ media, folders, onClose, onChanged }: Props) {
  const [playbackUrl, setPlaybackUrl] = useState<string | null>(null);
  const [name, setName] = useState(media.title);
  const [folderId, setFolderId] = useState(media.folderId ?? "");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    void backend.getMediaPlaybackPath(media.id)
      .then((path) => { if (!disposed) setPlaybackUrl(convertFileSrc(path)); })
      .catch((caught: unknown) => { if (!disposed) setError(caught instanceof Error ? caught.message : "Media file is unavailable."); });
    return () => { disposed = true; setPlaybackUrl(null); };
  }, [media]);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    try {
      if (name.trim() !== media.title) await backend.renameMedia(media.id, name);
      if ((folderId || null) !== media.folderId) await backend.moveMedia(media.id, folderId || null);
      onChanged();
      onClose();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Changes could not be saved.");
    }
  }

  async function remove() {
    if (!window.confirm(`Delete “${media.title}” from BITIG? This removes the managed file.`)) return;
    try {
      await backend.deleteMedia(media.id);
      onChanged();
      onClose();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Media could not be deleted.");
    }
  }

  async function exportMedia() {
    const destination = await saveDialog({
      title: "Export media",
      defaultPath: `${sanitizeExportName(media.title)}.${media.container}`,
      filters: [{ name: media.container.toUpperCase(), extensions: [media.container] }],
    });
    if (!destination) return;
    try {
      await backend.exportMedia(media.id, destination);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Media could not be exported.");
    }
  }

  async function reveal() {
    try {
      const path = await backend.getMediaPlaybackPath(media.id);
      await revealItemInDir(path);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Media could not be revealed.");
    }
  }

  return (
    <SidePanel title="Media details" open onClose={onClose}>
      <form className="media-detail" onSubmit={(event) => void save(event)}>
        <div className="player-shell">
          {playbackUrl && media.mediaType === "video" && <video controls src={playbackUrl} />}
          {playbackUrl && media.mediaType === "audio" && <div className="audio-player"><span>♫</span><audio controls src={playbackUrl} /></div>}
          {!playbackUrl && <div className="preview-placeholder">Loading media…</div>}
        </div>
        <div className="media-metadata"><span>{media.mediaType}</span><span>{media.container.toUpperCase()}</span><span>{media.height ? `${media.height}p` : "Audio"}</span><span>{formatDuration(media.durationMs)}</span></div>
        <label>Name<input value={name} onChange={(event) => setName(event.target.value)} maxLength={240} required /></label>
        <label>Folder<select value={folderId} onChange={(event) => setFolderId(event.target.value)}><option value="">All media</option>{folders.map((folder) => <option value={folder.id} key={folder.id}>{folder.name}</option>)}</select></label>
        {error && <p className="form-error" role="alert">{error}</p>}
        <div className="secondary-actions"><button type="button" onClick={() => void exportMedia()}>EXPORT</button><button type="button" onClick={() => void reveal()}>SHOW IN FOLDER</button></div>
        <div className="detail-actions"><button className="danger-button" type="button" onClick={() => void remove()}>DELETE</button><button className="primary-button" type="submit">SAVE CHANGES</button></div>
      </form>
    </SidePanel>
  );
}
