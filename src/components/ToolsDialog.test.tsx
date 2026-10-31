import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { initialToolUpdateView } from "../lib/toolUpdates";
import { ToolsDialog } from "./ToolsDialog";

it("shows media tools and their update controls without application updates", () => {
  const markup = renderToStaticMarkup(<ToolsDialog
    update={initialToolUpdateView} checking={false} autoEnabled={false} preferenceError={null}
    report={{ checkedAt: null, tools: ["yt-dlp", "FFmpeg", "ffprobe"].map((name) => ({
      id: name, name, installedVersion: "1.0", latestVersion: null, state: "not_checked",
      integrityVerified: true, updateSupported: true, message: null,
    })) }}
    onClose={vi.fn()} onOpenQuickTour={vi.fn()} onPreferenceChange={vi.fn()}
    onCheckUpdates={vi.fn()} onInstallUpdates={vi.fn()} onRestart={vi.fn()}
  />);
  for (const text of ["yt-dlp", "FFmpeg", "ffprobe", "CHECK UPDATES", "OPEN QUICK TOUR", "Legal / acceptable use",
    "Automatically check for and install media tool updates"]) expect(markup).toContain(text);
  expect(markup).not.toContain("APPLICATION UPDATES");
  expect(markup).not.toContain("CHECK FOR APP UPDATES");
});

it("keeps manual repair visible for blocked yt-dlp while automatic updates are off", () => {
  const message = "yt-dlp could not be verified as version 2026.10.26 or newer. Install updates, then restart BITIG.";
  const markup = renderToStaticMarkup(<ToolsDialog
    update={initialToolUpdateView} checking={false} autoEnabled={false} preferenceError={null}
    report={{ checkedAt: null, tools: [{
      id: "yt-dlp", name: "yt-dlp", installedVersion: "nightly", latestVersion: "2026.10.26",
      state: "update_available", integrityVerified: true, updateSupported: true, message,
    }] }}
    onClose={vi.fn()} onOpenQuickTour={vi.fn()} onPreferenceChange={vi.fn()}
    onCheckUpdates={vi.fn()} onInstallUpdates={vi.fn()} onRestart={vi.fn()}
  />);
  expect(markup).toContain(message);
  expect(markup).toContain("INSTALL UPDATES");
  expect(markup).not.toContain('checked=""');
});
