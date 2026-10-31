import { useEffect, useId, useRef, useState, type KeyboardEvent, type SyntheticEvent } from "react";
import { AppSelect } from "./AppSelect";
import {
  clampSeek,
  clearFullscreenControlsHide,
  formatPlaybackTime,
  isPlaybackInput,
  isPlaybackRate,
  PLAYBACK_RATES,
  playbackErrorMessage,
  playbackShortcut,
  readPlaybackState,
  scheduleFullscreenControlsHide,
  shouldSuppressFullscreenCancel,
  type FullscreenControlsAutoHideState,
} from "../lib/playback";

interface Props {
  src: string;
  poster?: string;
  title: string;
  error: string | null;
  onPlaybackError: (message: string | null) => void;
  theater: boolean;
  onToggleTheater: () => void;
}

type Icon = "play" | "pause" | "back" | "forward" | "volume" | "muted" | "theater" | "fullscreen" | "exit";

function PlayerIcon({ name }: { name: Icon }) {
  return <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true" focusable="false">
    {name === "play" && <path d="m8 5 11 7-11 7Z" fill="currentColor" stroke="none" />}
    {name === "pause" && <path d="M7 5v14M17 5v14" strokeWidth="4" />}
    {(name === "back" || name === "forward") && <g transform={name === "forward" ? "translate(24 0) scale(-1 1)" : undefined}><path d="M7 3 3 7l4 4M3 7h12l5 5v7" /><path d="M8 14v7m5-7h5v7h-5Z" transform={name === "forward" ? "translate(24 0) scale(-1 1)" : undefined} /></g>}
    {(name === "volume" || name === "muted") && <><path d="M3 9h4l5-5v16l-5-5H3Z" />{name === "muted" ? <path d="m16 9 6 6m0-6-6 6" /> : <path d="M16 8v8m4-11v14" />}</>}
    {name === "theater" && <><path d="M3 5h18v14H3Z" /><path d="M6 8h12v8H6Z" /></>}
    {name === "fullscreen" && <path d="M3 9V3h6m6 0h6v6M3 15v6h6m6 0h6v-6" />}
    {name === "exit" && <path d="M9 3v6H3m12-6v6h6M3 15h6v6m6 0v-6h6" />}
  </svg>;
}

export function MediaPlayer({ src, poster, title, error, onPlaybackError, theater, onToggleTheater }: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const controlsRef = useRef<HTMLDivElement>(null);
  const noticeId = useId();
  const [media, setMedia] = useState(() => readPlaybackState(null));
  const [fullscreen, setFullscreen] = useState(false);
  const [controlsHidden, setControlsHidden] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const autoHideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const interactionActive = useRef(false);
  const controlsFocused = useRef(false);
  const fullscreenActive = useRef(false);
  const fullscreenPending = useRef(false);
  const fullscreenExitedAt = useRef(-Infinity);

  function sync(event: SyntheticEvent<HTMLVideoElement>) {
    setMedia(readPlaybackState(event.currentTarget));
  }

  const resetAutoHideRef = useRef<() => void>(() => {});
  const getAutoHideStateRef = useRef<() => FullscreenControlsAutoHideState>(() => ({ fullscreen: false }));

  function getAutoHideState(): FullscreenControlsAutoHideState {
    const root = rootRef.current;
    const speedMenuOpen = Boolean(root?.querySelector('[role="listbox"]'));
    return {
      fullscreen: fullscreenActive.current,
      theater,
      controlInteractionActive: interactionActive.current,
      speedMenuOpen,
      controlsFocused: controlsFocused.current,
    };
  }

  function resetAutoHide() {
    setControlsHidden(false);
    if (!fullscreenActive.current) {
      clearFullscreenControlsHide(autoHideTimer);
      return;
    }
    scheduleFullscreenControlsHide(autoHideTimer, getAutoHideStateRef.current, () => {
      setControlsHidden(true);
    });
  }

  useEffect(() => {
    getAutoHideStateRef.current = getAutoHideState;
    resetAutoHideRef.current = resetAutoHide;
  });

  async function togglePlayback() {
    resetAutoHide();
    const video = videoRef.current;
    if (!video) return;
    if (!video.paused && !video.ended) {
      video.pause();
      return;
    }
    try {
      if (video.ended) video.currentTime = 0;
      await video.play();
    } catch (caught) {
      if (videoRef.current !== video || (caught instanceof DOMException && caught.name === "AbortError")) return;
      onPlaybackError(video.error ? playbackErrorMessage(video.error.code) : "Playback could not start. Try the play button again.");
    }
  }

  function seek(time: number) {
    resetAutoHide();
    const video = videoRef.current;
    if (!video) return;
    const position = clampSeek(time, video.duration);
    if (position !== null) video.currentTime = position;
  }

  function toggleMute() {
    resetAutoHide();
    const video = videoRef.current;
    if (!video) return;
    if (video.muted || video.volume === 0) {
      if (video.volume === 0) video.volume = 0.5;
      video.muted = false;
    } else video.muted = true;
  }

  async function toggleFullscreen() {
    const root = rootRef.current;
    if (!root || fullscreenPending.current) return;
    fullscreenPending.current = true;
    setNotice(null);
    try {
      if (document.fullscreenElement === root) await document.exitFullscreen();
      else await root.requestFullscreen({ navigationUI: "hide" });
      if (rootRef.current === root) root.focus({ preventScroll: true });
    } catch {
      if (rootRef.current === root) setNotice("Fullscreen is unavailable. You can use Theater Mode instead.");
    } finally {
      fullscreenPending.current = false;
    }
  }

  useEffect(() => {
    const root = rootRef.current;
    const video = videoRef.current;
    const dialog = root?.closest("dialog");
    const fullscreenChanged = () => {
      const active = document.fullscreenElement === root;
      if (fullscreenActive.current && !active) fullscreenExitedAt.current = performance.now();
      fullscreenActive.current = active;
      setFullscreen(active);
      if (active) {
        setControlsHidden(false);
        scheduleFullscreenControlsHide(autoHideTimer, getAutoHideStateRef.current, () => {
          setControlsHidden(true);
        });
      } else {
        clearFullscreenControlsHide(autoHideTimer);
        setControlsHidden(false);
      }
    };
    const cancel = (event: Event) => {
      // Chromium can dispatch dialog cancel after fullscreenchange for the same Escape.
      if (shouldSuppressFullscreenCancel(document.fullscreenElement === root || fullscreenActive.current, fullscreenExitedAt.current, performance.now())) {
        event.preventDefault();
        event.stopPropagation();
        if (document.fullscreenElement === root) void document.exitFullscreen().catch(() => undefined);
      }
    };
    const keyReleased = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") fullscreenExitedAt.current = -Infinity;
    };
    const handlePointerEnd = () => {
      if (interactionActive.current) {
        interactionActive.current = false;
        resetAutoHideRef.current();
      }
    };

    document.addEventListener("fullscreenchange", fullscreenChanged);
    document.addEventListener("keyup", keyReleased);
    window.addEventListener("pointerup", handlePointerEnd);
    window.addEventListener("pointercancel", handlePointerEnd);
    dialog?.addEventListener("cancel", cancel);
    return () => {
      document.removeEventListener("fullscreenchange", fullscreenChanged);
      document.removeEventListener("keyup", keyReleased);
      window.removeEventListener("pointerup", handlePointerEnd);
      window.removeEventListener("pointercancel", handlePointerEnd);
      dialog?.removeEventListener("cancel", cancel);
      clearFullscreenControlsHide(autoHideTimer);
      if (clickTimer.current) clearTimeout(clickTimer.current);
      video?.pause();
    };
  }, []);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    resetAutoHide();
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.key === "Tab" && document.fullscreenElement === rootRef.current) {
      const controls = rootRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled):not([tabindex="-1"]), input:not(:disabled)');
      const first = controls?.[0];
      const last = controls?.[controls.length - 1];
      if ((!event.shiftKey && event.target === last) || (event.shiftKey && (event.target === first || event.target === rootRef.current))) {
        event.preventDefault();
        (event.shiftKey ? last : first)?.focus();
      }
      return;
    }
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target && (isPlaybackInput(target.tagName, target.isContentEditable, target.getAttribute("role")) || target.closest("[role='listbox']"))) return;
    const action = playbackShortcut(event.key);
    if (!action || (event.key === " " && target?.closest("button"))) return;
    if (action === "exitFullscreen" && document.fullscreenElement !== rootRef.current) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    switch (action) {
      case "toggle": void togglePlayback(); break;
      case "back": seek((videoRef.current?.currentTime ?? 0) - 10); break;
      case "forward": seek((videoRef.current?.currentTime ?? 0) + 10); break;
      case "mute": toggleMute(); break;
      case "fullscreen": case "exitFullscreen": void toggleFullscreen(); break;
    }
  }

  const status = error ? "error" : media.status;
  const paused = media.paused || status === "ended";
  const muted = media.muted || media.volume === 0;
  const isHidden = fullscreen && controlsHidden;

  return (
    <div
      ref={rootRef}
      className={`media-player${isHidden ? " controls-hidden" : ""}`}
      data-media-player
      data-state={status}
      role="region"
      aria-label={`Video player: ${title}`}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      onPointerMove={resetAutoHide}
      onMouseMove={resetAutoHide}
    >
      <div className="player-stage">
        <video ref={videoRef} src={src} poster={poster} playsInline aria-label={title} aria-describedby={error || notice ? noticeId : undefined}
          onLoadStart={sync} onEmptied={sync} onLoadedMetadata={sync} onDurationChange={sync} onCanPlay={sync}
          onPlay={sync} onPlaying={(event) => { sync(event); onPlaybackError(null); }} onPause={sync}
          onTimeUpdate={sync} onSeeking={sync} onSeeked={sync} onWaiting={sync} onStalled={sync}
          onEnded={sync} onVolumeChange={sync} onRateChange={sync}
          onError={(event) => { sync(event); onPlaybackError(playbackErrorMessage(event.currentTarget.error?.code)); }} />
        <button className="player-surface" type="button" aria-label={paused ? "Play video" : "Pause video"} tabIndex={-1} onClick={(event) => {
          rootRef.current?.focus({ preventScroll: true });
          if (clickTimer.current) clearTimeout(clickTimer.current);
          if (event.detail > 1) return;
          if (event.detail === 0) void togglePlayback();
          else clickTimer.current = setTimeout(() => void togglePlayback(), 350);
        }} onDoubleClick={() => {
          if (clickTimer.current) clearTimeout(clickTimer.current);
          void toggleFullscreen();
        }}>
          {paused && <span className="player-center-play"><PlayerIcon name="play" /></span>}
        </button>
        <span className="player-status" role="status">{status}</span>
        {(error || notice) && <p className="player-notice" id={noticeId} role="alert">{error ?? notice}{error && " Use SHOW IN FOLDER or EXPORT from media details."}</p>}
      </div>
      <div
        ref={controlsRef}
        className="player-controls"
        onPointerDown={() => {
          interactionActive.current = true;
          resetAutoHide();
        }}
        onFocusCapture={() => {
          controlsFocused.current = true;
          resetAutoHide();
        }}
        onBlurCapture={(event) => {
          const nextTarget = event.relatedTarget as Node | null;
          if (!controlsRef.current?.contains(nextTarget)) {
            controlsFocused.current = false;
          }
          resetAutoHide();
        }}
      >
        <div className="player-timeline">
          <span>{formatPlaybackTime(media.currentTime)}</span>
          <input className="player-range player-seek" type="range" min={0} max={media.duration || 1} step="0.1" value={Math.min(media.currentTime, media.duration)} disabled={!media.duration || status === "error"} aria-label="Playback position" aria-valuetext={`${formatPlaybackTime(media.currentTime)} of ${media.duration ? formatPlaybackTime(media.duration) : "unknown duration"}`} onChange={(event) => seek(event.currentTarget.valueAsNumber)} />
          <span>{media.duration ? formatPlaybackTime(media.duration) : "—"}</span>
        </div>
        <div className="player-control-row">
          <div className="player-transport" role="group" aria-label="Playback controls">
            <button type="button" className="player-toggle" aria-label={paused ? "Play" : "Pause"} aria-keyshortcuts="Space K" title={paused ? "Play (Space / K)" : "Pause (Space / K)"} onClick={() => void togglePlayback()}><PlayerIcon name={paused ? "play" : "pause"} /></button>
            <button type="button" aria-label="Back 10 seconds" aria-keyshortcuts="ArrowLeft J" title="Back 10 seconds (← / J)" disabled={!media.duration || status === "error"} onClick={() => seek((videoRef.current?.currentTime ?? 0) - 10)}><PlayerIcon name="back" /></button>
            <button type="button" aria-label="Forward 10 seconds" aria-keyshortcuts="ArrowRight L" title="Forward 10 seconds (→ / L)" disabled={!media.duration || status === "error"} onClick={() => seek((videoRef.current?.currentTime ?? 0) + 10)}><PlayerIcon name="forward" /></button>
          </div>
          <div className="player-volume">
            <button type="button" aria-label={muted ? "Unmute" : "Mute"} aria-pressed={muted} aria-keyshortcuts="M" title={muted ? "Unmute (M)" : "Mute (M)"} onClick={toggleMute}><PlayerIcon name={muted ? "muted" : "volume"} /></button>
            <input className="player-range" type="range" min={0} max={1} step="0.01" value={muted ? 0 : media.volume} aria-label="Volume" aria-valuetext={`${Math.round((muted ? 0 : media.volume) * 100)}%`} onChange={(event) => {
              const video = videoRef.current;
              if (video) { video.volume = event.currentTarget.valueAsNumber; video.muted = false; }
            }} />
          </div>
          <div className="player-view-controls">
            <AppSelect ariaLabel="Playback speed" value={String(media.rate)} options={PLAYBACK_RATES.map((rate) => ({ value: String(rate), label: `${rate}×` }))} onChange={(value) => { const rate = Number(value); if (videoRef.current && isPlaybackRate(rate)) videoRef.current.playbackRate = rate; }} />
            <button type="button" aria-label={theater ? "Exit Theater Mode" : "Theater Mode"} aria-pressed={theater} title={theater ? "Back to media details" : "Theater Mode"} disabled={fullscreen} onClick={onToggleTheater}><PlayerIcon name="theater" /></button>
            <button type="button" aria-label={fullscreen ? "Exit fullscreen" : "Fullscreen"} aria-pressed={fullscreen} aria-keyshortcuts="F" title={fullscreen ? "Exit fullscreen (F / Esc)" : "Fullscreen (F)"} onClick={() => void toggleFullscreen()}><PlayerIcon name={fullscreen ? "exit" : "fullscreen"} /></button>
          </div>
        </div>
      </div>
    </div>
  );
}
