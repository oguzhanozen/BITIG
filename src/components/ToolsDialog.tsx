import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { backend, displayError } from "../services/backend";
import type { ToolStatusReport, ToolUpdateProgress, ToolUpdateState } from "../types/domain";

interface ToolsDialogProps {
  onClose: () => void;
  onOpenQuickTour: () => void;
}

const stateLabels: Record<ToolUpdateState, string> = {
  not_checked: "NOT CHECKED",
  current: "UP TO DATE",
  update_available: "UPDATE AVAILABLE",
  check_failed: "CHECK FAILED",
};

function progressLabel(progress: ToolUpdateProgress): string {
  switch (progress.stage) {
    case "checking":
      return "Checking official releases…";
    case "downloading":
      return "Downloading " + (progress.toolName ?? "tools") + "…";
    case "verifying":
      return "Verifying " + (progress.toolName ?? "downloaded tools") + "…";
    case "installing":
      return "Installing verified tools…";
  }
}

export function ToolsDialog({ onClose, onOpenQuickTour }: ToolsDialogProps) {
  const [report, setReport] = useState<ToolStatusReport | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [refreshingStatus, setRefreshingStatus] = useState(false);
  const [restartRequired, setRestartRequired] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ToolUpdateProgress | null>(null);
  const statusRequestId = useRef(0);
  const installingRef = useRef(false);
  const mountedRef = useRef(false);
  const progressUnlistenRef = useRef<(() => void) | null>(null);
  const hasUpdates = report?.tools.some((tool) => tool.state === "update_available") ?? false;
  const downloadPercentage = !refreshingStatus && progress?.stage === "downloading" && progress.totalBytes > 0
    ? Math.max(0, Math.min(100, Math.floor(progress.downloadedBytes / progress.totalBytes * 100)))
    : null;
  const progressText = refreshingStatus ? "Refreshing tool status…" : progress ? progressLabel(progress) : "";

  useEffect(() => {
    mountedRef.current = true;
    const requestId = ++statusRequestId.current;
    void backend.getToolStatus(false).then((value) => {
      if (mountedRef.current && requestId === statusRequestId.current) setReport(value);
    }).catch((caught: unknown) => {
      if (mountedRef.current && requestId === statusRequestId.current) {
        setError(displayError(caught, "Tool status could not be loaded."));
      }
    });
    return () => {
      mountedRef.current = false;
      statusRequestId.current += 1;
      progressUnlistenRef.current?.();
      progressUnlistenRef.current = null;
    };
  }, []);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !installingRef.current) onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  async function checkUpdates() {
    const requestId = ++statusRequestId.current;
    setChecking(true);
    setError(null);
    try {
      const nextReport = await backend.getToolStatus(true);
      if (mountedRef.current && requestId === statusRequestId.current) setReport(nextReport);
    } catch (caught) {
      if (mountedRef.current && requestId === statusRequestId.current) {
        setError(displayError(caught, "Updates could not be checked."));
      }
    } finally {
      if (mountedRef.current) setChecking(false);
    }
  }

  async function installUpdates() {
    if (installingRef.current) return;
    const operationId = crypto.randomUUID();
    installingRef.current = true;
    statusRequestId.current += 1;
    setInstalling(true);
    setError(null);
    setNotice(null);
    setProgress({
      operationId,
      stage: "checking",
      toolName: null,
      downloadedBytes: 0,
      totalBytes: 0,
    });
    let unlisten: (() => void) | null = null;
    let stopped = false;
    const stopListening = () => {
      if (stopped) return;
      stopped = true;
      unlisten?.();
    };
    try {
      unlisten = await listen<ToolUpdateProgress>("tool-update://progress", ({ payload }) => {
        if (mountedRef.current && payload.operationId === operationId) setProgress(payload);
      });
      if (!mountedRef.current) return;
      progressUnlistenRef.current = stopListening;
      const result = await backend.installToolUpdates(operationId);
      if (!mountedRef.current) return;
      setRestartRequired(result.restartRequired);
      setNotice(result.updatedTools.length > 0
        ? `${result.updatedTools.join(" & ")} installed and verified. Restart BITIG to use them.`
        : "The installed tools are already up to date.");
      installingRef.current = false;
      setInstalling(false);
      setRefreshingStatus(true);
      stopListening();
      if (progressUnlistenRef.current === stopListening) progressUnlistenRef.current = null;
      setChecking(true);
      try {
        const requestId = ++statusRequestId.current;
        const nextReport = await backend.getToolStatus(true);
        if (mountedRef.current && requestId === statusRequestId.current) setReport(nextReport);
      } catch {
        if (mountedRef.current) setError("Tools were installed, but their status could not be refreshed.");
      } finally {
        if (mountedRef.current) {
          setChecking(false);
          setRefreshingStatus(false);
        }
      }
    } catch (caught) {
      if (mountedRef.current) {
        setError(displayError(caught, "Tool updates could not be installed."));
      }
    } finally {
      stopListening();
      if (progressUnlistenRef.current === stopListening) progressUnlistenRef.current = null;
      installingRef.current = false;
      if (mountedRef.current) {
        setInstalling(false);
        setRefreshingStatus(false);
        setProgress(null);
      }
    }
  }

  async function restart() {
    setError(null);
    try {
      await backend.restartApp();
    } catch (caught) {
      setError(displayError(caught, "BITIG could not restart. Close and reopen it to use the updated tools."));
    }
  }

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !installingRef.current) onClose();
    }}>
      <section className="center-modal tools-dialog" role="dialog" aria-modal="true" aria-labelledby="tools-title">
        <div className="modal-heading">
          <div><p className="eyebrow">ABOUT &amp; TOOLS</p><h2 id="tools-title">BITIG</h2></div>
          <button className="popover-close" type="button" onClick={onClose} aria-label="Close tools" disabled={installing}>×</button>
        </div>

        {(installing || refreshingStatus) && progress && (
          <div className="tool-update-feedback">
            <div className="tool-update-heading">
              <p aria-live="polite">{progressText}</p>
              {downloadPercentage !== null && <strong aria-hidden="true">{downloadPercentage}%</strong>}
            </div>
            <div
              className={"tool-update-track" + (downloadPercentage === null ? " is-indeterminate" : "")}
              role="progressbar"
              aria-label="Tool update progress"
              aria-valuetext={progressText + (downloadPercentage === null ? "" : " " + downloadPercentage + "% downloaded")}
              aria-valuemin={downloadPercentage === null ? undefined : 0}
              aria-valuemax={downloadPercentage === null ? undefined : 100}
              aria-valuenow={downloadPercentage ?? undefined}
            >
              <span style={downloadPercentage === null ? undefined : { width: downloadPercentage + "%" }} />
            </div>
            {!refreshingStatus && progress.stage === "downloading" && <p>Verification and installation follow the download.</p>}
          </div>
        )}
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
          {!report && !error && <p className="tool-loading">Reading bundled tool status…</p>}
        </div>

        <article className="tool-row legal-notice">
          <div className="tool-row-heading"><h3>Legal / acceptable use</h3></div>
          <p>BITIG should only be used for content you have the right to download, copy, or store. Users are responsible for complying with applicable copyright law and terms of service. BITIG is not designed to bypass DRM or access controls.</p>
        </article>

        {error && <p className="form-error" role="alert">{error}</p>}
        {notice && <p className="form-success" role="status">{notice}</p>}
        {report?.checkedAt && <p className="checked-at">Last checked {new Date(report.checkedAt).toLocaleString()}</p>}

        <div className="tools-actions">
          {restartRequired ? (
            <button className="primary-button" type="button" onClick={() => void restart()} disabled={installing}>RESTART BITIG</button>
          ) : hasUpdates ? (
            <button className="primary-button" type="button" onClick={() => void installUpdates()} disabled={installing || checking}>{installing ? "UPDATING TOOLS…" : "INSTALL UPDATES"}</button>
          ) : (
            <button className="primary-button" type="button" onClick={() => void checkUpdates()} disabled={checking || installing}>{checking ? "CHECKING…" : "CHECK UPDATES"}</button>
          )}
          {hasUpdates && !restartRequired && <button className="secondary-button" type="button" onClick={() => void checkUpdates()} disabled={checking || installing}>{checking ? "CHECKING…" : "CHECK AGAIN"}</button>}
          <button className="secondary-button" type="button" onClick={onOpenQuickTour} disabled={installing}>OPEN QUICK TOUR</button>
        </div>
      </section>
    </div>
  );
}
