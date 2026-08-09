import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { UrlAnalyzer } from "./components/UrlAnalyzer";
import { MediaGrid } from "./components/MediaGrid";
import { Sidebar } from "./components/Sidebar";
import { DownloadsPanel } from "./components/DownloadsPanel";
import { MediaDetailDialog } from "./components/MediaDetailDialog";
import { QuickTour } from "./components/QuickTour";
import { TitleBar } from "./components/TitleBar";
import { ToolsDialog } from "./components/ToolsDialog";
import type { Media } from "./types/domain";
import { useLibrary } from "./hooks/useLibrary";
import { useDownloads } from "./hooks/useDownloads";
import { isPaletteId, type PaletteId, type ThemeMode } from "./theme";

export default function App() {
  const library = useLibrary();
  const downloads = useDownloads();
  const [search, setSearch] = useState("");
  const [folderOpen, setFolderOpen] = useState(false);
  const [folderName, setFolderName] = useState("");
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [selectedMedia, setSelectedMedia] = useState<Media | null>(null);
  const [retryRequest, setRetryRequest] = useState({ key: 0, url: "" });
  const [downloadsPanelSignal, setDownloadsPanelSignal] = useState(0);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [quickTourOpen, setQuickTourOpen] = useState(
    () => localStorage.getItem("bitig.quickTour.completed") !== "true",
  );
  const [theme, setTheme] = useState<ThemeMode>(() => {
    const saved = localStorage.getItem("bitig.theme");
    if (saved === "light" || saved === "dark") return saved;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });
  const [palette, setPalette] = useState<PaletteId>(() => {
    const saved = localStorage.getItem("bitig.palette");
    return isPaletteId(saved) ? saved : "stormy-morning";
  });

  const activeFolderName = useMemo(
    () => library.folders.find((folder) => folder.id === library.selectedFolderId)?.name ?? "All media",
    [library.folders, library.selectedFolderId],
  );
  const completedDownloads = downloads.jobs.filter((job) => job.status === "completed").length;
  const reloadLibrary = library.reload;

  useEffect(() => {
    if (completedDownloads > 0) void reloadLibrary();
  }, [completedDownloads, reloadLibrary]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("bitig.theme", theme);
  }, [theme]);

  useEffect(() => {
    document.documentElement.dataset.palette = palette;
    localStorage.setItem("bitig.palette", palette);
  }, [palette]);

  const closeFolderCreator = useCallback(() => {
    setFolderOpen(false);
    setDialogError(null);
  }, []);

  const closeQuickTour = useCallback(() => {
    localStorage.setItem("bitig.quickTour.completed", "true");
    setQuickTourOpen(false);
  }, []);

  async function submitFolder(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setDialogError(null);
    try {
      await library.createFolder(folderName, library.selectedFolderId);
      setFolderName("");
      setFolderOpen(false);
    } catch (error) {
      setDialogError(typeof error === "object" && error !== null && "message" in error ? String(error.message) : "Folder could not be created.");
    }
  }

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void library.search(search);
  }

  return (
    <div className="app-frame">
      <TitleBar />
      <div className="app-shell">
      <Sidebar
        folders={library.folders}
        selectedFolderId={library.selectedFolderId}
        onSelect={(id) => void library.selectFolder(id)}
        search={search}
        onSearchChange={setSearch}
        onSearchSubmit={submitSearch}
        folderCreatorOpen={folderOpen}
        folderName={folderName}
        folderError={dialogError}
        onNewFolder={() => {
          setDialogError(null);
          setFolderOpen((current) => !current);
        }}
        onCloseFolderCreator={closeFolderCreator}
        onFolderNameChange={setFolderName}
        onSubmitFolder={(event) => void submitFolder(event)}
        onDeleteFolder={(folder) => {
          if (!window.confirm(`Delete “${folder.name}”? Only empty folders can be deleted.`)) return;
          setDialogError(null);
          void library.deleteFolder(folder.id).catch((error: unknown) => {
            setDialogError(error instanceof Error ? error.message : "Folder could not be deleted. Make sure it is empty.");
          });
        }}
        activeDownloads={downloads.activeCount}
        onOpenDownloads={() => setDownloadsPanelSignal((current) => current + 1)}
        onOpenTools={() => setToolsOpen(true)}
        theme={theme}
        onThemeChange={setTheme}
        palette={palette}
        onPaletteChange={setPalette}
      />
      <main className="main-content">
        <div className="content-area">
          <UrlAnalyzer key={retryRequest.key} initialUrl={retryRequest.url} folders={library.folders} onStarted={downloads.add} />
          {library.error && <div className="error-banner" role="alert"><strong>Couldn’t open the vault.</strong> {library.error.message}<button type="button" onClick={() => void library.reload()}>Try again</button></div>}
          <MediaGrid items={library.media} folders={library.folders} loading={library.loading} folderName={activeFolderName} onAddUrl={() => document.getElementById("media-url")?.focus()} onSelect={setSelectedMedia} onChanged={() => void library.reload()} />
        </div>
      </main>
      </div>

      <DownloadsPanel jobs={downloads.jobs} openSignal={downloadsPanelSignal} onCancel={(id) => void downloads.cancel(id)} onRetry={(url) => {
        setRetryRequest((current) => ({ key: current.key + 1, url }));
        document.getElementById("media-url")?.scrollIntoView({ behavior: "smooth", block: "center" });
      }} />
      {selectedMedia && <MediaDetailDialog key={selectedMedia.id} media={selectedMedia} folders={library.folders} onClose={() => setSelectedMedia(null)} onChanged={() => void library.reload()} />}
      {toolsOpen && <ToolsDialog onClose={() => setToolsOpen(false)} onOpenQuickTour={() => {
        setToolsOpen(false);
        setQuickTourOpen(true);
      }} />}
      {quickTourOpen && <QuickTour onClose={closeQuickTour} />}
    </div>
  );
}
