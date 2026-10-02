import { useEffect, useRef, useState } from "react";
import { displayError } from "../services/backend";
import type { DownloadJob } from "../types/domain";

interface DownloadsPanelProps {
  jobs: DownloadJob[];
  openSignal: number;
  onCancel: (id: string) => Promise<unknown>;
  onRetry: (sourceUrl: string) => void;
}

const terminal = new Set(["completed", "failed", "cancelled"]);

function formatRate(bytesPerSecond: number | null): string | null {
  if (!bytesPerSecond || bytesPerSecond <= 0) return null;
  return `${(bytesPerSecond / 1024 / 1024).toFixed(1)} MB/s`;
}

export function DownloadsPanel({ jobs, openSignal, onCancel, onRetry }: DownloadsPanelProps) {
  const [manualVisible, setManualVisible] = useState(false);
  const [dismissedActiveIds, setDismissedActiveIds] = useState<string[]>([]);
  const [actionError, setActionError] = useState<string | null>(null);
  const manualHideTimer = useRef<number | null>(null);
  const activeIds = jobs.filter((job) => !terminal.has(job.status)).map((job) => job.id);
  const visible = manualVisible || activeIds.some((id) => !dismissedActiveIds.includes(id));

  function closePanel() {
    setActionError(null);
    setDismissedActiveIds(activeIds);
    if (manualHideTimer.current !== null) window.clearTimeout(manualHideTimer.current);
    manualHideTimer.current = null;
    setManualVisible(false);
  }

  async function cancelDownload(id: string) {
    setActionError(null);
    try {
      await onCancel(id);
    } catch (error) {
      setActionError(displayError(error, "This download could not be cancelled."));
    }
  }

  useEffect(() => {
    if (openSignal <= 0) return;
    const openTimer = window.setTimeout(() => {
      setActionError(null);
      if (manualHideTimer.current !== null) window.clearTimeout(manualHideTimer.current);
      setManualVisible(true);
      manualHideTimer.current = window.setTimeout(() => {
        manualHideTimer.current = null;
        setManualVisible(false);
      }, 4_000);
    }, 0);
    return () => window.clearTimeout(openTimer);
  }, [openSignal]);

  useEffect(() => () => {
    if (manualHideTimer.current !== null) window.clearTimeout(manualHideTimer.current);
  }, []);

  if (!visible) return null;
  return (
    <aside className="downloads-panel" aria-label="Downloads">
      <div className="downloads-heading">
        <div><p className="eyebrow">DOWNLOAD MANAGER</p><h2>Recent activity</h2></div>
        <div className="downloads-heading-actions">{jobs.length > 0 && <span>{jobs.length}</span>}<button type="button" onClick={closePanel} aria-label="Close download manager">×</button></div>
      </div>
      <div className="download-list">
        {actionError && <p className="download-action-error" role="alert">{actionError}</p>}
        {jobs.length === 0 && (
          <div className="downloads-empty" role="status">
            <strong>No downloads in the queue yet.</strong>
            <p>Start a download to see its progress here.</p>
          </div>
        )}
        {jobs.slice(0, 5).map((job) => (
          <article className="download-row" key={job.id}>
            <div className="download-row-heading"><h3>{job.title}</h3><strong data-status={job.status}>{job.status.replace("_", " ")}</strong></div>
            {!terminal.has(job.status) && (
              <div className="download-progress-row">
                <div className={`progress-track ${job.status === "downloading" && job.totalBytes ? "" : "is-indeterminate"}`}>
                  <span style={job.status === "downloading" && job.totalBytes ? { width: `${Math.round(job.progress * 100)}%` } : undefined} />
                </div>
                {job.status === "downloading" && job.totalBytes && <small>{Math.round(job.progress * 100)}%</small>}
              </div>
            )}
            {formatRate(job.bytesPerSecond) && <p className="download-rate">{formatRate(job.bytesPerSecond)}</p>}
            {job.status === "failed" && <p>This download could not be completed. You can try again.</p>}
            {!terminal.has(job.status) && <button type="button" onClick={() => void cancelDownload(job.id)}>Cancel</button>}
            {(job.status === "failed" || job.status === "cancelled") && <button type="button" onClick={() => onRetry(job.sourceUrl)}>Try again</button>}
          </article>
        ))}
      </div>
    </aside>
  );
}
