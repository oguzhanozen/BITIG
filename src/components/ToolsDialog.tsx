import { useEffect, useState } from "react";
import { backend } from "../services/backend";
import type { ToolStatusReport, ToolUpdateState } from "../types/domain";

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

export function ToolsDialog({ onClose, onOpenQuickTour }: ToolsDialogProps) {
  const [report, setReport] = useState<ToolStatusReport | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [restartRequired, setRestartRequired] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const hasUpdates = report?.tools.some((tool) => tool.state === "update_available") ?? false;

  useEffect(() => {
    let disposed = false;
    void backend.getToolStatus(false).then((value) => {
      if (!disposed) setReport(value);
    }).catch((caught: unknown) => {
      if (!disposed) setError(caught instanceof Error ? caught.message : "Tool status could not be loaded.");
    });
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      disposed = true;
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [onClose]);

  async function checkUpdates() {
    setChecking(true);
    setError(null);
    try {
      setReport(await backend.getToolStatus(true));
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Updates could not be checked.");
    } finally {
      setChecking(false);
    }
  }

  async function installUpdates() {
    setInstalling(true);
    setError(null);
    setNotice(null);
    try {
      const result = await backend.installToolUpdates();
      setRestartRequired(result.restartRequired);
      setNotice(result.updatedTools.length > 0
        ? `${result.updatedTools.join(" & ")} installed and verified. Restart BITIG to use them.`
        : "The installed tools are already up to date.");
      setReport(await backend.getToolStatus(true));
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Tool updates could not be installed.");
    } finally {
      setInstalling(false);
    }
  }

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section className="center-modal tools-dialog" role="dialog" aria-modal="true" aria-labelledby="tools-title">
        <div className="modal-heading">
          <div><p className="eyebrow">ABOUT &amp; TOOLS</p><h2 id="tools-title">BITIG</h2></div>
          <button className="popover-close" type="button" onClick={onClose} aria-label="Close tools">×</button>
        </div>

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
            <button className="primary-button" type="button" onClick={() => void backend.restartApp()}>RESTART BITIG</button>
          ) : hasUpdates ? (
            <button className="primary-button" type="button" onClick={() => void installUpdates()} disabled={installing || checking}>{installing ? "DOWNLOADING & VERIFYING…" : "INSTALL UPDATES"}</button>
          ) : (
            <button className="primary-button" type="button" onClick={() => void checkUpdates()} disabled={checking || installing}>{checking ? "CHECKING…" : "CHECK UPDATES"}</button>
          )}
          {hasUpdates && !restartRequired && <button className="secondary-button" type="button" onClick={() => void checkUpdates()} disabled={checking || installing}>{checking ? "CHECKING…" : "CHECK AGAIN"}</button>}
          <button className="secondary-button" type="button" onClick={onOpenQuickTour}>OPEN QUICK TOUR</button>
        </div>
      </section>
    </div>
  );
}
