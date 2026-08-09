import { useCallback, useEffect, useRef, useState } from "react";
import type { DownloadJob } from "../types/domain";

interface DownloadsPanelProps {
  jobs: DownloadJob[];
  openSignal: number;
  onCancel: (id: string) => void;
  onRetry: (sourceUrl: string) => void;
}

const terminal = new Set(["completed", "failed", "cancelled"]);

function formatRate(bytesPerSecond: number | null): string | null {
  if (!bytesPerSecond || bytesPerSecond <= 0) return null;
  return `${(bytesPerSecond / 1024 / 1024).toFixed(1)} MB/s`;
}

export function DownloadsPanel({ jobs, openSignal, onCancel, onRetry }: DownloadsPanelProps) {
  const [visible, setVisible] = useState(false);
  const hideTimer = useRef<number | null>(null);
  const activityFingerprint = jobs
    .map((job) => `${job.id}:${job.status}:${job.downloadedBytes ?? 0}`)
    .join("|");

  const previousFingerprint = useRef(activityFingerprint);

  const showBriefly = useCallback(() => {
    setVisible(true);
    if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => setVisible(false), 3_000);
  }, []);

  useEffect(() => {
    if (previousFingerprint.current === activityFingerprint) return;
    previousFingerprint.current = activityFingerprint;
    showBriefly();
    return () => {
      if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    };
  }, [activityFingerprint, showBriefly]);

  useEffect(() => {
    if (openSignal <= 0) return;
    const openTimer = window.setTimeout(showBriefly, 0);
    return () => window.clearTimeout(openTimer);
  }, [openSignal, showBriefly]);

  if (!jobs.length || !visible) return null;
  return (
    <aside className="downloads-panel" aria-label="Downloads">
      <div className="downloads-heading">
        <div><p className="eyebrow">DOWNLOAD MANAGER</p><h2>Recent activity</h2></div>
        <div className="downloads-heading-actions"><span>{jobs.length}</span><button type="button" onClick={() => setVisible(false)} aria-label="Close download manager">×</button></div>
      </div>
      <div className="download-list">
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
            {job.errorMessage && <p>{job.errorMessage}</p>}
            {!terminal.has(job.status) && <button type="button" onClick={() => onCancel(job.id)}>Cancel</button>}
            {(job.status === "failed" || job.status === "cancelled") && <button type="button" onClick={() => onRetry(job.sourceUrl)}>Try again</button>}
          </article>
        ))}
      </div>
    </aside>
  );
}
