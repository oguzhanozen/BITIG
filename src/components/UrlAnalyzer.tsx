import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { backend, displayError } from "../services/backend";
import type { DownloadJob, Folder, MediaAnalysis } from "../types/domain";
import { buildFolderOptions, formatDuration } from "../lib/format";
import { folderNameError } from "../lib/folders";
import { AppSelect } from "./AppSelect";

interface UrlAnalyzerProps {
  folders: Folder[];
  initialUrl?: string;
  onCreateFolder: (name: string, parentId: string | null) => Promise<Folder>;
  onStarted: (job: DownloadJob) => void;
}

function formatBytes(bytes: number | null): string {
  if (bytes === null) return "Size unknown";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `~${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

export function UrlAnalyzer({ folders, initialUrl = "", onCreateFolder, onStarted }: UrlAnalyzerProps) {
  const [url, setUrl] = useState(initialUrl);
  const [analysis, setAnalysis] = useState<MediaAnalysis | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [folderId, setFolderId] = useState("");
  const [loading, setLoading] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [folderCreatorOpen, setFolderCreatorOpen] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const [folderError, setFolderError] = useState<string | null>(null);
  const [creatingFolder, setCreatingFolder] = useState(false);
  const creatingFolderRef = useRef(false);
  const folderInputRef = useRef<HTMLInputElement>(null);
  const folderButtonRef = useRef<HTMLButtonElement>(null);
  const folderOptions = useMemo(() => buildFolderOptions(folders), [folders]);

  const selectedOption = useMemo(
    () => analysis?.options.find((option) => option.id === selectedId) ?? null,
    [analysis, selectedId],
  );

  useEffect(() => {
    if (folderCreatorOpen) folderInputRef.current?.focus();
  }, [folderCreatorOpen]);

  function closeFolderCreator() {
    setFolderCreatorOpen(false);
    setNewFolderName("");
    setFolderError(null);
    window.requestAnimationFrame(() => folderButtonRef.current?.focus());
  }

  async function createFolder(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (creatingFolderRef.current || loading || starting) return;
    const validationError = folderNameError(newFolderName);
    setFolderError(validationError);
    if (validationError) {
      folderInputRef.current?.focus();
      return;
    }
    creatingFolderRef.current = true;
    setCreatingFolder(true);
    try {
      const folder = await onCreateFolder(newFolderName.trim(), folderId || null);
      setFolderId(folder.id);
      closeFolderCreator();
    } catch (caught) {
      setFolderError(displayError(caught, "Folder could not be created."));
    } finally {
      creatingFolderRef.current = false;
      setCreatingFolder(false);
    }
  }

  async function analyze(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!url.trim()) {
      setError("Paste a media link to analyze.");
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const result = await backend.analyzeUrl(url);
      setAnalysis(result);
      setName(result.title);
      setSelectedId(result.options[0]?.id ?? null);
    } catch (caught) {
      setAnalysis(null);
      setError(displayError(caught, "This link could not be analyzed."));
    } finally {
      setLoading(false);
    }
  }

  async function startDownload() {
    if (!analysis || !selectedOption || !name.trim() || creatingFolderRef.current) return;
    setStarting(true);
    setError(null);
    try {
      const job = await backend.startDownload({
        analysisId: analysis.analysisId,
        optionId: selectedOption.id,
        title: name,
        folderId: folderId || null,
      });
      onStarted(job);
      setAnalysis(null);
      setSelectedId(null);
      setName("");
      setFolderId("");
      setUrl("");
      setFolderCreatorOpen(false);
      setNewFolderName("");
      setFolderError(null);
    } catch (caught) {
      setError(displayError(caught, "The download could not be started."));
    } finally {
      setStarting(false);
    }
  }

  return (
    <section className="url-workbench" aria-labelledby="url-workbench-title">
      <div className="workbench-heading">
        <div>
          <p className="eyebrow">ADD TO LIBRARY</p>
          <h1 id="url-workbench-title">Bring a link. Keep the media.</h1>
        </div>
      </div>

      <form className="url-form" onSubmit={(event) => void analyze(event)} autoComplete="off" noValidate>
        <label className="sr-only" htmlFor="media-url">Media URL</label>
        <span aria-hidden="true">↗</span>
        <input id="media-url" type="url" value={url} onChange={(event) => setUrl(event.target.value)} placeholder="Paste a media URL…" autoComplete="off" required disabled={loading || starting || creatingFolder} />
        <button className="primary-button" type="submit" disabled={loading || starting || creatingFolder || !url.trim()}>
          {loading ? "CHECKING…" : "ANALYZE"}
        </button>
      </form>
      <p className="field-help">Only save media you own or have permission to download.</p>
      {error && <p className="form-error" role="alert">{error}</p>}

      {analysis && (
        <div className="analysis-result">
          {analysis.existingVersions > 0 && (
            <p className="duplicate-note">
              You already keep {analysis.existingVersions} {analysis.existingVersions === 1 ? "version" : "versions"} of this source. You can still choose another format.
            </p>
          )}
          <div className="analysis-preview">
            {analysis.thumbnailUrl ? <img src={analysis.thumbnailUrl} alt="" /> : <div className="preview-placeholder">▶</div>}
            <div className="analysis-copy">
              <p>{analysis.sourcePlatform}</p>
              <h2>{analysis.title}</h2>
              <span>{analysis.creator ?? "Unknown creator"} · {formatDuration(analysis.durationMs)}</span>
            </div>
            <button className="quiet-button" type="button" disabled={creatingFolder} onClick={() => { setAnalysis(null); closeFolderCreator(); }}>Clear</button>
          </div>

          <div className="option-columns">
            <div className="format-section">
              <p className="format-label">VIDEO</p>
              <div className="format-grid">
                {analysis.options.filter((option) => option.kind === "video").map((option) => (
                  <button key={option.id} className={`format-option ${selectedId === option.id ? "is-selected" : ""}`} type="button" onClick={() => setSelectedId(option.id)}>
                    <strong>{option.resolution}p</strong><span>{option.container.toUpperCase()} · {formatBytes(option.estimatedSize)}</span>
                  </button>
                ))}
              </div>
            </div>
            <div className="format-section">
              <p className="format-label">AUDIO</p>
              <div className="format-grid audio-options">
                {analysis.options.filter((option) => option.kind === "audio").map((option) => (
                  <button key={option.id} className={`format-option ${selectedId === option.id ? "is-selected" : ""}`} type="button" onClick={() => setSelectedId(option.id)}>
                    <strong>{option.container.toUpperCase()}</strong><span>{formatBytes(option.estimatedSize)}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="download-fields">
            <label htmlFor="download-name">Name<input id="download-name" value={name} onChange={(event) => setName(event.target.value)} maxLength={240} autoComplete="off" /></label>
            <div className="download-folder-field">
              <label htmlFor="download-folder">Folder<AppSelect id="download-folder" ariaLabel="Folder" value={folderId} disabled={creatingFolder} options={[{ value: "", label: "All media" }, ...folderOptions]} onChange={setFolderId} /></label>
              <button ref={folderButtonRef} className="secondary-button" type="button" disabled={loading || starting || creatingFolder} aria-expanded={folderCreatorOpen} aria-controls="download-folder-creator" onClick={() => setFolderCreatorOpen(true)}>+ NEW FOLDER</button>
            </div>
            <button className="primary-button start-download" type="button" disabled={!selectedOption || !name.trim() || starting || creatingFolder} onClick={() => void startDownload()}>{starting ? "STARTING…" : "DOWNLOAD"}</button>
          </div>
          {folderCreatorOpen && (
            <form id="download-folder-creator" className="download-folder-creator" aria-label="Create download folder" aria-busy={creatingFolder} autoComplete="off" noValidate onSubmit={(event) => void createFolder(event)} onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                if (!creatingFolderRef.current) closeFolderCreator();
              }
            }}>
              <p className="field-help" id="download-folder-parent">Create in: <strong>{folderOptions.find((option) => option.value === folderId)?.label ?? "All media"}</strong>. Up to 120 characters.</p>
              <label htmlFor="download-new-folder-name">Folder name
                <input ref={folderInputRef} id="download-new-folder-name" className="text-input" value={newFolderName} onChange={(event) => { setNewFolderName(event.target.value); setFolderError(null); }} onBlur={() => setFolderError(folderNameError(newFolderName))} disabled={creatingFolder || loading || starting} required aria-invalid={Boolean(folderError)} aria-describedby={`download-folder-parent${folderError ? " download-folder-error" : ""}`} />
              </label>
              <div className="download-folder-actions">
                <button className="primary-button" type="submit" disabled={creatingFolder || loading || starting}>{creatingFolder ? "CREATING…" : "CREATE"}</button>
                <button className="secondary-button" type="button" disabled={creatingFolder} onClick={closeFolderCreator}>CANCEL</button>
              </div>
              {folderError && <p className="form-error" id="download-folder-error" role="alert">{folderError}</p>}
            </form>
          )}
        </div>
      )}
    </section>
  );
}
