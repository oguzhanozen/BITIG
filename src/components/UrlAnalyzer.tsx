import { useMemo, useState, type FormEvent } from "react";
import { backend } from "../services/backend";
import type { DownloadJob, Folder, MediaAnalysis } from "../types/domain";
import { formatDuration } from "../lib/format";

interface UrlAnalyzerProps {
  folders: Folder[];
  initialUrl?: string;
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

export function UrlAnalyzer({ folders, initialUrl = "", onStarted }: UrlAnalyzerProps) {
  const [url, setUrl] = useState(initialUrl);
  const [analysis, setAnalysis] = useState<MediaAnalysis | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [folderId, setFolderId] = useState("");
  const [loading, setLoading] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedOption = useMemo(
    () => analysis?.options.find((option) => option.id === selectedId) ?? null,
    [analysis, selectedId],
  );

  async function analyze(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setLoading(true);
    setError(null);
    try {
      const result = await backend.analyzeUrl(url);
      setAnalysis(result);
      setName(result.title);
      setSelectedId(result.options[0]?.id ?? null);
    } catch (caught) {
      setAnalysis(null);
      setError(caught instanceof Error ? caught.message : "This URL could not be analyzed.");
    } finally {
      setLoading(false);
    }
  }

  async function startDownload() {
    if (!analysis || !selectedOption || !name.trim()) return;
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
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "The download could not be started.");
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

      <form className="url-form" onSubmit={(event) => void analyze(event)}>
        <label className="sr-only" htmlFor="media-url">Media URL</label>
        <span aria-hidden="true">↗</span>
        <input id="media-url" type="url" value={url} onChange={(event) => setUrl(event.target.value)} placeholder="Paste a media URL…" autoComplete="off" required disabled={loading || starting} />
        <button className="primary-button" type="submit" disabled={loading || starting || !url.trim()}>
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
            <button className="quiet-button" type="button" onClick={() => setAnalysis(null)}>Clear</button>
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
            <label htmlFor="download-name">Name<input id="download-name" value={name} onChange={(event) => setName(event.target.value)} maxLength={240} /></label>
            <label htmlFor="download-folder">Folder<select id="download-folder" value={folderId} onChange={(event) => setFolderId(event.target.value)}><option value="">All media</option>{folders.map((folder) => <option key={folder.id} value={folder.id}>{folder.name}</option>)}</select></label>
            <button className="primary-button start-download" type="button" disabled={!selectedOption || !name.trim() || starting} onClick={() => void startDownload()}>{starting ? "STARTING…" : "DOWNLOAD"}</button>
          </div>
        </div>
      )}
    </section>
  );
}
