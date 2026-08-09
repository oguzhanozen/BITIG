import { useEffect, useState, type MouseEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

const appWindow = "__TAURI_INTERNALS__" in window ? getCurrentWindow() : null;

export function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!appWindow) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = () => {
      void appWindow.isMaximized().then((value) => {
        if (!disposed) setMaximized(value);
      });
    };
    refresh();
    void appWindow.onResized(refresh).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  function toggleMaximize(event?: MouseEvent<HTMLElement>) {
    if (!appWindow) return;
    if (event?.target instanceof Element && event.target.closest("button")) return;
    void appWindow.toggleMaximize().then(() => appWindow.isMaximized()).then(setMaximized);
  }

  return (
    <header className="title-bar" data-tauri-drag-region onDoubleClick={toggleMaximize}>
      <div className="title-bar-drag" data-tauri-drag-region aria-hidden="true" />
      <div className="window-controls">
        <button type="button" onClick={() => void appWindow?.minimize()} aria-label="Minimize" title="Minimize">
          <span className="minimize-icon" aria-hidden="true" />
        </button>
        <button type="button" onClick={() => toggleMaximize()} aria-label={maximized ? "Restore" : "Maximize"} title={maximized ? "Restore" : "Maximize"}>
          <span className={maximized ? "restore-icon" : "maximize-icon"} aria-hidden="true" />
        </button>
        <button className="close-window" type="button" onClick={() => void appWindow?.close()} aria-label="Close" title="Close">
          <span className="close-icon" aria-hidden="true" />
        </button>
      </div>
    </header>
  );
}
