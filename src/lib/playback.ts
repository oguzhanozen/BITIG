import { formatDuration } from "./format";

export function shouldSuppressFullscreenCancel(fullscreen: boolean, lastExit: number, now: number): boolean {
  return fullscreen || (now >= lastExit && now - lastExit < 250);
}

export const FULLSCREEN_IDLE_HIDE_MS = 3000;

export interface FullscreenControlsAutoHideState {
  fullscreen: boolean;
  theater?: boolean;
  controlInteractionActive?: boolean;
  speedMenuOpen?: boolean;
  controlsFocused?: boolean;
}

export interface TimerHolder {
  current: ReturnType<typeof setTimeout> | null;
}

export function canAutoHideFullscreenControls(state: FullscreenControlsAutoHideState): boolean {
  if (!state.fullscreen) return false;
  if (state.controlInteractionActive) return false;
  if (state.speedMenuOpen) return false;
  if (state.controlsFocused) return false;
  return true;
}

export function clearFullscreenControlsHide(timerRef: TimerHolder): void {
  if (timerRef.current !== null) {
    clearTimeout(timerRef.current);
    timerRef.current = null;
  }
}

export function scheduleFullscreenControlsHide(
  timerRef: TimerHolder,
  getState: () => FullscreenControlsAutoHideState,
  onHide: () => void,
): void {
  clearFullscreenControlsHide(timerRef);
  if (!canAutoHideFullscreenControls(getState())) return;
  timerRef.current = setTimeout(() => {
    timerRef.current = null;
    if (canAutoHideFullscreenControls(getState())) {
      onHide();
    }
  }, FULLSCREEN_IDLE_HIDE_MS);
}

type PlaybackSource = Pick<HTMLMediaElement, "currentTime" | "duration" | "volume" | "muted" | "playbackRate" | "paused" | "ended" | "seeking" | "readyState"> & { error: Pick<MediaError, "code"> | null };

export function readPlaybackState(media: PlaybackSource | null) {
  const currentTime = media && Number.isFinite(media.currentTime) ? Math.max(0, media.currentTime) : 0;
  const duration = media && Number.isFinite(media.duration) && media.duration > 0 ? media.duration : 0;
  const status = media?.error ? "error" : media?.seeking ? "seeking" : media?.ended ? "ended"
    : !media || media.readyState < 2 || (!media.paused && media.readyState < 3) ? "loading"
    : !media.paused ? "playing" : currentTime > 0 ? "paused" : "ready";
  return { status, currentTime, duration, volume: media?.volume ?? 1, muted: media?.muted ?? false, rate: media?.playbackRate ?? 1, paused: media?.paused ?? true };
}

export const PLAYBACK_RATES = [1, 1.25, 1.5, 2] as const;

export function isPlaybackRate(rate: number): boolean {
  return PLAYBACK_RATES.some((value) => value === rate);
}

export function formatPlaybackTime(seconds: number): string {
  return formatDuration(Number.isFinite(seconds) && seconds >= 0 ? seconds * 1000 : null);
}

export function clampSeek(time: number, duration: number): number | null {
  if (!Number.isFinite(time) || !Number.isFinite(duration) || duration <= 0) return null;
  return Math.max(0, Math.min(time, duration));
}

export function isPlaybackInput(tagName: string, editable: boolean, role: string | null): boolean {
  return editable || ["input", "textarea", "select"].includes(tagName.toLowerCase()) || role === "combobox" || role === "listbox";
}

export function playbackShortcut(key: string): "toggle" | "back" | "forward" | "mute" | "fullscreen" | "exitFullscreen" | null {
  switch (key.toLowerCase()) {
    case " ": case "k": return "toggle";
    case "arrowleft": case "j": return "back";
    case "arrowright": case "l": return "forward";
    case "m": return "mute";
    case "f": return "fullscreen";
    case "escape": return "exitFullscreen";
    default: return null;
  }
}

export function playbackErrorMessage(code: number | null | undefined): string {
  switch (code) {
    case 1:
      return "Playback was interrupted. Try playing the media again.";
    case 2:
      return "The media file could not be loaded.";
    case 3:
      return "The built-in player could not read this media.";
    case 4:
      return "This media format is not supported by the built-in player.";
    default:
      return "This media could not be played.";
  }
}
