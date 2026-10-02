import { useEffect, useState, type FormEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { backend, displayError } from "../services/backend";
import type { Folder, Media } from "../types/domain";
import { formatDuration, sanitizeExportName } from "../lib/format";
import { SidePanel } from "./SidePanel";
import type { Confirmation } from "./ConfirmDialog";
import { AppSelect } from "./AppSelect";

interface Props {
  media: Media;
  folders: Folder[];
  onClose: () => void;
  onChanged: () => void;
  requestConfirmation: (details: Confirmation) => Promise<boolean>;
}

export function MediaDetailDialog({ media, folders, onClose, onChanged, requestConfirmation }: Props) {
  const [playbackUrl, setPlaybackUrl] = useState<string | null>(null);
  const [name, setName] = useState(media.title);
  const [folderId, setFolderId] = useState(media.folderId ?? "");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    void backend.getMediaPlaybackPath(media.id)
      .then((path) => { if (!disposed) setPlaybackUrl(convertFileSrc(path)); })
      .catch((caught: unknown) => { if (!disposed) setError(displayError(caught, "Media file is unavailable.")); });
    return () => { disposed = true; setPlaybackUrl(null); };
  }, [media]);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    if (!name.trim()) {
      setError("Enter a name for this media.");
      return;
    }
    try {
      if (name.trim() !== media.title) await backend.renameMedia(media.id, name);
      if ((folderId || null) !== media.folderId) await backend.moveMedia(media.id, folderId || null);
      onChanged();
      onClose();
    } catch (caught) {
      setError(displayError(caught, "Changes could not be saved."));
    }
  }

  async function remove() {
    if (!await requestConfirmation({
      title: "Delete media?",
      message: `Delete “${media.title}” from BITIG? This removes the managed file.`,
      confirmLabel: "DELETE MEDIA",
    })) return;
    try {
      await backend.deleteMedia(media.id);
      onChanged();
      onClose();
    } catch (caught) {
      setError(displayError(caught, "Media could not be deleted."));
    }
  }

  async function exportMedia() {
    try {
      const destination = await saveDialog({
        title: "Export media",
        defaultPath: `${sanitizeExportName(media.title)}.${media.container}`,
        filters: [{ name: media.container.toUpperCase(), extensions: [media.container] }],
      });
      if (!destination) return;
      await backend.exportMedia(media.id, destination);
    } catch (caught) {
      setError(displayError(caught, "Media could not be exported."));
    }
  }

  async function reveal() {
    try {
      const path = await backend.getMediaPlaybackPath(media.id);
      await revealItemInDir(path);
    } catch (caught) {
      setError(displayError(caught, "Media could not be revealed."));
    }
  }

  return (
    <SidePanel title="Media details" open onClose={onClose}>
      <form className="media-detail" onSubmit={(event) => void save(event)} autoComplete="off" noValidate>
        <div className="player-shell">
          {playbackUrl && media.mediaType === "video" && <video controls src={playbackUrl} />}
          {playbackUrl && media.mediaType === "audio" && <div className="audio-player"><span>♫</span><audio controls src={playbackUrl} /></div>}
          {!playbackUrl && <div className="preview-placeholder">Loading media…</div>}
        </div>
        <div className="media-metadata"><span>{media.mediaType}</span><span>{media.container.toUpperCase()}</span><span>{media.height ? `${media.height}p` : "Audio"}</span><span>{formatDuration(media.durationMs)}</span></div>
        <label>Name<input value={name} onChange={(event) => setName(event.target.value)} maxLength={240} autoComplete="off" /></label>
        <label>Folder<AppSelect ariaLabel="Folder" value={folderId} options={[{ value: "", label: "All media" }, ...folders.map((folder) => ({ value: folder.id, label: folder.name }))]} onChange={setFolderId} /></label>
        {error && <p className="form-error" role="alert">{error}</p>}
        <div className="secondary-actions"><button type="button" onClick={() => void exportMedia()}>EXPORT</button><button type="button" onClick={() => void reveal()}>SHOW IN FOLDER</button></div>
        <div className="detail-actions"><button className="danger-button" type="button" onClick={() => void remove()}>DELETE</button><button className="primary-button" type="submit">SAVE CHANGES</button></div>
      </form>
    </SidePanel>
  );
}
