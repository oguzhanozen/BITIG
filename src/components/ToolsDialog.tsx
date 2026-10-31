import { useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";
import type { ToolStatusReport, ToolUpdateProgress, ToolUpdateState } from "../types/domain";
import { toolUpdateProgressLabel, type ToolUpdateView } from "../lib/toolUpdates";

interface ToolsDialogProps {
  onClose: () => void;
  onOpenQuickTour: () => void;
  update: ToolUpdateView;
  report: ToolStatusReport | null;
  checking: boolean;
  autoEnabled: boolean;
  preferenceError: string | null;
  onPreferenceChange: (enabled: boolean) => void;
  onCheckUpdates: () => void;
  onInstallUpdates: () => void;
  onRestart: () => void;
}

const stateLabels: Record<ToolUpdateState, string> = {
  not_checked: "NOT CHECKED",
  current: "UP TO DATE",
  update_available: "UPDATE AVAILABLE",
  check_failed: "CHECK FAILED",
};

export function ToolUpdateProgressDisplay({ progress, refreshingStatus }: { progress: ToolUpdateProgress; refreshingStatus: boolean }) {
  const percentage = !refreshingStatus && progress.stage === "downloading" && progress.totalBytes > 0
    ? Math.max(0, Math.min(100, Math.floor(progress.downloadedBytes / progress.totalBytes * 100))) : null;
  const text = refreshingStatus ? "Refreshing tool status…" : toolUpdateProgressLabel(progress);
  return (
    <div className="tool-update-feedback">
      <div className="tool-update-heading"><p aria-live="polite">{text}</p>{percentage !== null && <strong aria-hidden="true">{percentage}%</strong>}</div>
      <div className={"tool-update-track" + (percentage === null ? " is-indeterminate" : "")}
        role="progressbar" aria-label="Tool update progress" aria-valuetext={text + (percentage === null ? "" : " " + percentage + "% downloaded")}
        aria-valuemin={percentage === null ? undefined : 0} aria-valuemax={percentage === null ? undefined : 100} aria-valuenow={percentage ?? undefined}>
        <span style={percentage === null ? undefined : { width: percentage + "%" }} />
      </div>
      {!refreshingStatus && progress.stage === "downloading" && <p>Verification and installation follow the download.</p>}
    </div>
  );
}

export function ToolsDialog({ onClose, onOpenQuickTour, update, report, checking, autoEnabled, preferenceError, onPreferenceChange, onCheckUpdates, onInstallUpdates, onRestart }: ToolsDialogProps) {
  const { installing, refreshingStatus, progress, restartRequired, notice, error } = update;
  const hasUpdates = report?.tools.some((tool) => tool.state === "update_available") ?? false;
  const sectionRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const origin = document.activeElement;
    sectionRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => {
      if (origin instanceof HTMLElement && origin.isConnected && origin !== document.body) origin.focus();
      else document.querySelector<HTMLButtonElement>(".tools-trigger")?.focus();
    };
  }, []);
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section ref={sectionRef} className="center-modal tools-dialog" role="dialog" aria-modal="true" aria-labelledby="tools-title" onKeyDown={(event) => {
        if (event.key !== "Tab") return;
        const controls = sectionRef.current?.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), a[href]");
        const first = controls?.[0];
        const last = controls?.[controls.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }}>
        <div className="modal-heading">
          <div><p className="eyebrow">ABOUT &amp; TOOLS</p><h2 id="tools-title">BITIG</h2></div>
          <button className="popover-close" type="button" onClick={onClose} aria-label="Close tools">×</button>
        </div>

        {(installing || refreshingStatus) && progress && <ToolUpdateProgressDisplay progress={progress} refreshingStatus={refreshingStatus} />}
        <div className="tool-list">
          {report?.tools.map((tool) => (
            <article className="tool-row" key={tool.id}>
              <div className="tool-row-heading"><h3>{tool.name}</h3><strong data-state={tool.state}>{stateLabels[tool.state]}</strong></div>
              <dl>
                <div><dt>INSTALLED</dt><dd>{tool.installedVersion}</dd></div>
                <div><dt>LATEST</dt><dd>{tool.latestVersion ?? "—"}</dd></div>
                <div><dt>INTEGRITY</dt><dd>{tool.integrityVerified ? "VERIFIED" : "UNKNOWN"}</dd></div>
                <div><dt>UPDATES</dt><dd>{tool.updateSupported ? "IN APP" : "WITH BITIG"}</dd></div>
              </dl>
              {tool.message && <p>{tool.message}</p>}
            </article>
          ))}
          {!report && !error && <p className="tool-loading">Reading tool status…</p>}
        </div>

        <article className="tool-row tool-update-setting">
          <label><input type="checkbox" checked={autoEnabled} onChange={(event) => onPreferenceChange(event.currentTarget.checked)} />
            <span>Automatically check for and install media tool updates</span>
          </label>
          <p>Only yt-dlp, FFmpeg and ffprobe. This setting does not update BITIG itself. You can always check for and install tool updates manually.</p>
          {preferenceError && <p className="form-error" role="alert">{preferenceError}</p>}
        </article>

        <article className="tool-row legal-notice">
          <div className="tool-row-heading"><h3>Legal / acceptable use</h3></div>
          <p>BITIG should only be used for content you have the right to download, copy, or store. Users are responsible for complying with applicable copyright law and terms of service. BITIG is not designed to bypass DRM or access controls.</p>
        </article>

        {error && <p className="form-error" role="alert">{error}</p>}
        {notice && <p className="form-success" role="status">{notice}</p>}
        {report?.checkedAt && <p className="checked-at">Last checked {new Date(report.checkedAt).toLocaleString()}</p>}

        <div className="tools-actions">
          {restartRequired ? (
            <button className="primary-button" type="button" onClick={onRestart} disabled={installing}>RESTART BITIG</button>
          ) : hasUpdates ? (
            <button className="primary-button" type="button" onClick={onInstallUpdates} disabled={installing || checking}>{installing ? "UPDATING TOOLS…" : "INSTALL UPDATES"}</button>
          ) : (
            <button className="primary-button" type="button" onClick={onCheckUpdates} disabled={checking || installing}>{checking ? "CHECKING…" : "CHECK UPDATES"}</button>
          )}
          {hasUpdates && !restartRequired && <button className="secondary-button" type="button" onClick={onCheckUpdates} disabled={checking || installing}>{checking ? "CHECKING…" : "CHECK AGAIN"}</button>}
          <button className="secondary-button" type="button" onClick={onOpenQuickTour} disabled={installing}>OPEN QUICK TOUR</button>
        </div>
      </section>
    </div>
  );
}

export function ToolUpdateConsentDialog({ error, onDecide }: {
  error: string | null;
  onDecide: (preference: "enabled" | "disabled") => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const declineRef = useRef<HTMLButtonElement>(null);
  const enableRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const dialog = dialogRef.current;
    const origin = document.activeElement;
    if (!dialog?.open) dialog?.showModal();
    declineRef.current?.focus();
    return () => {
      dialog?.close();
      if (origin instanceof HTMLElement && origin.isConnected) origin.focus();
    };
  }, []);

  function handleKeys(event: ReactKeyboardEvent<HTMLDialogElement>) {
    if (event.key === "Escape") event.stopPropagation();
    if (event.key === "Tab") {
      if (event.shiftKey && document.activeElement === declineRef.current) {
        event.preventDefault(); enableRef.current?.focus();
      } else if (!event.shiftKey && document.activeElement === enableRef.current) {
        event.preventDefault(); declineRef.current?.focus();
      }
    }
  }

  return <dialog ref={dialogRef} className="center-modal tool-consent-dialog" aria-labelledby="tool-consent-title" aria-describedby="tool-consent-description"
    onKeyDown={handleKeys} onCancel={(event) => { event.preventDefault(); onDecide("disabled"); }}
    onClick={(event) => {
      if (event.target !== event.currentTarget) return;
      const bounds = event.currentTarget.getBoundingClientRect();
      if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) onDecide("disabled");
    }}>
    <div className="modal-heading"><div><p className="eyebrow">MEDIA TOOLS</p><h2 id="tool-consent-title">Keep your media tools up to date?</h2></div></div>
    <div className="tool-consent-copy" id="tool-consent-description">
      <p>Verified versions of these third-party tools come with BITIG, so you can use the bundled tools offline.</p>
      <dl className="tool-consent-tools">
        <div><dt>yt-dlp</dt><dd>Analyzes media links and downloads content.</dd></div>
        <div><dt>FFmpeg</dt><dd>Processes video and audio files.</dd></div>
        <div><dt>ffprobe</dt><dd>Inspects technical media details.</dd></div>
      </dl>
      <p>Optional automatic updates check official GitHub sources when BITIG opens. Downloaded files are verified with SHA-256 before installation. You decide when to restart BITIG.</p>
      <p>Automatic updates are off unless you enable them. You can change this preference later in About &amp; Tools. It is separate from installation terms.</p>
    </div>
    {error && <p className="form-error" role="alert">{error}</p>}
    <div className="modal-actions">
      <button ref={declineRef} className="secondary-button" type="button" onClick={() => onDecide("disabled")}>NOT NOW</button>
      <button ref={enableRef} className="primary-button" type="button" onClick={() => onDecide("enabled")}>ENABLE AUTOMATIC UPDATES</button>
    </div>
  </dialog>;
}
