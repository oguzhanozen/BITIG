import { useEffect, useMemo, useState, type FormEvent, type SyntheticEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { backend, displayError } from "../services/backend";
import type { Folder, Media } from "../types/domain";
import { buildFolderOptions, formatDuration, sanitizeExportName } from "../lib/format";
import { playbackErrorMessage } from "../lib/playback";
import { SidePanel } from "./SidePanel";
import type { Confirmation } from "./ConfirmDialog";
import { AppSelect } from "./AppSelect";
import { MediaPlayer } from "./MediaPlayer";

interface Props {
  media: Media;
  folders: Folder[];
  onClose: () => void;
  onChanged: () => void;
  requestConfirmation: (details: Confirmation) => Promise<boolean>;
}

export function MediaDetailDialog({ media, folders, onClose, onChanged, requestConfirmation }: Props) {
  const [playbackUrl, setPlaybackUrl] = useState<string | null>(null);
  const [playbackError, setPlaybackError] = useState<string | null>(null);
  const [posterUrl, setPosterUrl] = useState<string | null>(null);
  const [theater, setTheater] = useState(false);
  const [name, setName] = useState(media.title);
  const [folderId, setFolderId] = useState(media.folderId ?? "");
  const [error, setError] = useState<string | null>(null);
  const folderOptions = useMemo(() => buildFolderOptions(folders), [folders]);

  useEffect(() => {
    let disposed = false;
    void backend.getMediaPlaybackPath(media.id)
      .then((path) => { if (!disposed) setPlaybackUrl(convertFileSrc(path)); })
      .catch((caught: unknown) => { if (!disposed) setPlaybackError(displayError(caught, "Media file is unavailable.")); });
    return () => { disposed = true; setPlaybackUrl(null); };
  }, [media]);

  useEffect(() => {
    if (media.mediaType !== "video" || !media.thumbnailPath) return;
    let disposed = false;
    const image = new Image();
    void backend.getMediaThumbnailPath(media.id)
      .then((path) => {
        if (disposed || !path) return;
        const url = convertFileSrc(path);
        image.onload = () => { if (!disposed) setPosterUrl(url); };
        image.src = url;
      })
      .catch(() => undefined);
    return () => { disposed = true; image.onload = null; };
  }, [media.id, media.mediaType, media.thumbnailPath]);

  function handlePlaybackError(event: SyntheticEvent<HTMLMediaElement>) {
    setPlaybackError(playbackErrorMessage(event.currentTarget.error?.code));
  }

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
    <SidePanel title={theater ? "BITIG Theater" : "Media details"} open onClose={onClose} className={theater ? "media-theater" : undefined} onCancel={theater ? () => setTheater(false) : onClose}>
      <form className="media-detail" onSubmit={(event) => void save(event)} autoComplete="off" noValidate>
        {playbackUrl && media.mediaType === "video" && <MediaPlayer key={media.id} src={playbackUrl} poster={posterUrl ?? undefined} title={media.title} error={playbackError} onPlaybackError={setPlaybackError} theater={theater} onToggleTheater={() => setTheater((current) => !current)} />}
        {(!playbackUrl || media.mediaType === "audio") && <div className="player-shell">
          {playbackUrl && media.mediaType === "audio" && <div className="audio-player"><span>♫</span><audio controls src={playbackUrl} onError={handlePlaybackError} onPlaying={() => setPlaybackError(null)} aria-describedby={playbackError ? "playback-error" : undefined} /></div>}
          {!playbackUrl && <div className="preview-placeholder">{playbackError ? "Playback unavailable" : "Loading media…"}</div>}
        </div>}
        {playbackError && <p className="form-error" id="playback-error" role="alert">{playbackError} You can use SHOW IN FOLDER to open the file in another player, or EXPORT to save a copy.</p>}
        <div className="media-metadata"><span>{media.mediaType}</span><span>{media.container.toUpperCase()}</span><span>{media.height ? `${media.height}p` : "Audio"}</span><span>{formatDuration(media.durationMs)}</span></div>
        <label>Name<input value={name} onChange={(event) => setName(event.target.value)} maxLength={240} autoComplete="off" /></label>
        <label>Folder<AppSelect ariaLabel="Folder" value={folderId} options={[{ value: "", label: "All media" }, ...folderOptions]} onChange={setFolderId} /></label>
        {error && <p className="form-error" role="alert">{error}</p>}
        <div className="secondary-actions"><button type="button" onClick={() => void exportMedia()}>EXPORT</button><button type="button" onClick={() => void reveal()}>SHOW IN FOLDER</button></div>
        <div className="detail-actions"><button className="danger-button" type="button" onClick={() => void remove()}>DELETE</button><button className="primary-button" type="submit">SAVE CHANGES</button></div>
      </form>
    </SidePanel>
  );
}
