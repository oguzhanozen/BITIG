import { afterEach, describe, expect, it, vi } from "vitest";
import {
  canAutoHideFullscreenControls,
  clampSeek,
  clearFullscreenControlsHide,
  formatPlaybackTime,
  FULLSCREEN_IDLE_HIDE_MS,
  isPlaybackInput,
  isPlaybackRate,
  playbackErrorMessage,
  playbackShortcut,
  readPlaybackState,
  scheduleFullscreenControlsHide,
  shouldSuppressFullscreenCancel,
  type FullscreenControlsAutoHideState,
  type TimerHolder,
} from "./playback";

it("keeps Escape from closing the panel during fullscreen exit while allowing normal Escape afterwards", () => {
  expect(shouldSuppressFullscreenCancel(true, -Infinity, 1000)).toBe(true);
  expect(shouldSuppressFullscreenCancel(false, 1000, 1010)).toBe(true);
  expect(shouldSuppressFullscreenCancel(false, 1000, 1300)).toBe(false);
  expect(shouldSuppressFullscreenCancel(false, -Infinity, 1000)).toBe(false);
});

it("reads playback state from media events and starts a new media item cleanly", () => {
  const media = { currentTime: 10, duration: 90, volume: 0.5, muted: false, playbackRate: 1.25, paused: false, ended: false, seeking: false, readyState: 4, error: null };
  expect(readPlaybackState(media)).toMatchObject({ status: "playing", currentTime: 10, duration: 90, volume: 0.5, rate: 1.25, paused: false });
  expect(readPlaybackState({ ...media, paused: true }).status).toBe("paused");
  expect(readPlaybackState({ ...media, seeking: true }).status).toBe("seeking");
  expect(readPlaybackState({ ...media, ended: true }).status).toBe("ended");
  expect(readPlaybackState({ ...media, readyState: 1 }).status).toBe("loading");
  expect(readPlaybackState({ ...media, error: { code: 3 } }).status).toBe("error");
  expect(readPlaybackState({ ...media, currentTime: 0, paused: true }).status).toBe("ready");
  expect(readPlaybackState({ ...media, duration: Infinity, currentTime: NaN })).toMatchObject({ duration: 0, currentTime: 0 });
  expect(readPlaybackState(null)).toMatchObject({ status: "loading", currentTime: 0, duration: 0, rate: 1, paused: true, muted: false });
});

it("accepts the four supported playback speeds and rejects other values", () => {
  for (const rate of [1, 1.25, 1.5, 2]) expect(isPlaybackRate(rate)).toBe(true);
  for (const rate of [0, -1, 3, NaN, Infinity]) expect(isPlaybackRate(rate)).toBe(false);
});

it("formats player seconds without displaying non-finite or negative times", () => {
  expect(formatPlaybackTime(0)).toBe("0:00");
  expect(formatPlaybackTime(83.9)).toBe("1:23");
  expect(formatPlaybackTime(3723)).toBe("1:02:03");
  for (const time of [-1, NaN, Infinity]) expect(formatPlaybackTime(time)).toBe("—");
});

describe("seeking", () => {
  it("clamps positions and refuses unknown or invalid durations and positions", () => {
    expect(clampSeek(-10, 90)).toBe(0);
    expect(clampSeek(100, 90)).toBe(90);
    expect(clampSeek(45.5, 90)).toBe(45.5);
    for (const duration of [0, -1, NaN, Infinity]) expect(clampSeek(10, duration)).toBeNull();
    expect(clampSeek(NaN, 90)).toBeNull();
    expect(clampSeek(Infinity, 90)).toBeNull();
  });
});

describe("player keyboard commands", () => {
  it("leaves text fields, sliders, editable content and comboboxes to their own keyboard handlers", () => {
    expect(isPlaybackInput("INPUT", false, null)).toBe(true);
    expect(isPlaybackInput("textarea", false, null)).toBe(true);
    expect(isPlaybackInput("SELECT", false, null)).toBe(true);
    expect(isPlaybackInput("SPAN", true, null)).toBe(true);
    expect(isPlaybackInput("BUTTON", false, "combobox")).toBe(true);
    expect(isPlaybackInput("DIV", false, "listbox")).toBe(true);
    expect(isPlaybackInput("BUTTON", false, null)).toBe(false);
    expect(isPlaybackInput("DIV", false, null)).toBe(false);
  });
  it("maps only the player shortcuts, including uppercase letters", () => {
    expect([" ", "K", "ArrowLeft", "j", "ArrowRight", "L", "m", "F", "Escape", "Enter", "x"].map(playbackShortcut))
      .toEqual(["toggle", "toggle", "back", "back", "forward", "forward", "mute", "fullscreen", "exitFullscreen", null, null]);
  });
});

describe("playbackErrorMessage", () => {
  it.each([
    ["MEDIA_ERR_ABORTED", 1, "Playback was interrupted. Try playing the media again."],
    ["MEDIA_ERR_NETWORK", 2, "The media file could not be loaded."],
    ["MEDIA_ERR_DECODE", 3, "The built-in player could not read this media."],
    ["MEDIA_ERR_SRC_NOT_SUPPORTED", 4, "This media format is not supported by the built-in player."],
  ] as const)("maps %s to a user-facing message", (_name, code, message) => {
    expect(playbackErrorMessage(code)).toBe(message);
  });

  it.each([undefined, null, 0, -1, 5, 99, NaN, Infinity])("uses a safe fallback for code %s", (code) => {
    expect(playbackErrorMessage(code)).toBe("This media could not be played.");
  });
});

describe("fullscreen controls auto-hide", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts visible in fullscreen and hides after 3 seconds of inactivity", () => {
    vi.useFakeTimers();
    const timerRef: TimerHolder = { current: null };
    let hidden = false;
    const state: FullscreenControlsAutoHideState = { fullscreen: true };

    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });

    expect(FULLSCREEN_IDLE_HIDE_MS).toBe(3000);
    expect(hidden).toBe(false);
    vi.advanceTimersByTime(2999);
    expect(hidden).toBe(false);
    vi.advanceTimersByTime(1);
    expect(hidden).toBe(true);
  });

  it("shows controls on mouse movement or interaction and resets the 3s inactivity timer", () => {
    vi.useFakeTimers();
    const timerRef: TimerHolder = { current: null };
    let hidden = false;
    const state: FullscreenControlsAutoHideState = { fullscreen: true };
    const revealAndReset = () => {
      hidden = false;
      scheduleFullscreenControlsHide(timerRef, () => state, () => {
        hidden = true;
      });
    };

    revealAndReset();
    vi.advanceTimersByTime(2000);
    expect(hidden).toBe(false);

    // User moves mouse or interacts at t=2000ms -> resets 3000ms countdown
    revealAndReset();
    vi.advanceTimersByTime(2000);
    expect(hidden).toBe(false);
    vi.advanceTimersByTime(1000);
    expect(hidden).toBe(true);

    // Mouse moves after hidden -> immediately visible again, hides after another 3000ms
    revealAndReset();
    expect(hidden).toBe(false);
    vi.advanceTimersByTime(3000);
    expect(hidden).toBe(true);
  });

  it("does not hide during active seek/volume interaction, open speed menu, or keyboard focus in controls", () => {
    vi.useFakeTimers();
    const timerRef: TimerHolder = { current: null };
    let hidden = false;
    let state: FullscreenControlsAutoHideState = {
      fullscreen: true,
      controlInteractionActive: true,
    };

    expect(canAutoHideFullscreenControls(state)).toBe(false);
    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });
    vi.advanceTimersByTime(5000);
    expect(hidden).toBe(false);

    state = { fullscreen: true, speedMenuOpen: true };
    expect(canAutoHideFullscreenControls(state)).toBe(false);
    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });
    vi.advanceTimersByTime(5000);
    expect(hidden).toBe(false);

    state = { fullscreen: true, controlsFocused: true };
    expect(canAutoHideFullscreenControls(state)).toBe(false);
    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });
    vi.advanceTimersByTime(5000);
    expect(hidden).toBe(false);

    // Even if timer was scheduled before focus/menu/interaction became active, callback re-checks guards
    state = { fullscreen: true };
    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });
    vi.advanceTimersByTime(1500);
    state.speedMenuOpen = true;
    vi.advanceTimersByTime(2000);
    expect(hidden).toBe(false);
  });

  it("clears the timer when exiting fullscreen and never auto-hides in Normal or Theater Mode", () => {
    vi.useFakeTimers();
    const timerRef: TimerHolder = { current: null };
    let hidden = false;
    let state: FullscreenControlsAutoHideState = { fullscreen: false, theater: false };

    expect(canAutoHideFullscreenControls(state)).toBe(false);
    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });
    vi.advanceTimersByTime(5000);
    expect(hidden).toBe(false);

    state = { fullscreen: false, theater: true };
    expect(canAutoHideFullscreenControls(state)).toBe(false);
    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });
    vi.advanceTimersByTime(5000);
    expect(hidden).toBe(false);

    state = { fullscreen: true, theater: true };
    scheduleFullscreenControlsHide(timerRef, () => state, () => {
      hidden = true;
    });
    vi.advanceTimersByTime(1500);
    clearFullscreenControlsHide(timerRef);
    state = { fullscreen: false, theater: true };
    vi.advanceTimersByTime(3000);
    expect(hidden).toBe(false);
    expect(timerRef.current).toBeNull();
  });
});
