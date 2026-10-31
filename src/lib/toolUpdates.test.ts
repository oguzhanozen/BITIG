import { describe, expect, it, vi } from "vitest";
import {
  connectToolProgress, createToolUpdateSession, initialToolUpdateView, isToolUpdateConflict,
  initializeToolUpdatePreference, readToolUpdatePreference, runToolUpdate, saveToolUpdatePreference,
  TOOL_UPDATE_PREFERENCE_KEY, TOOL_UPDATE_RESTART_NOTICE,
  type ToolUpdatePreference, type ToolUpdateView,
} from "./toolUpdates";
import type { ToolUpdateProgress, ToolUpdateResult } from "../types/domain";

function storage(value: string | null = null) {
  const values = new Map<string, string>();
  if (value !== null) values.set(TOOL_UPDATE_PREFERENCE_KEY, value);
  return {
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => { values.set(key, value); }),
  };
}

describe("automatic update preference", () => {
  it("starts undecided and preserves all three explicit choices across reads", () => {
    const saved = storage();
    expect(readToolUpdatePreference(() => saved)).toEqual({ preference: "undecided", error: null });
    for (const preference of ["enabled", "disabled", "undecided"] as const) {
      expect(saveToolUpdatePreference(preference, () => saved)).toBeNull();
      expect(saved.setItem).toHaveBeenLastCalledWith("bitig.toolUpdates.auto", preference);
      expect(readToolUpdatePreference(() => saved)).toEqual({ preference, error: null });
    }
  });

  it.each(["true", "false", "ENABLED", "", "{broken", '"enabled"'])("keeps automatic updates off for invalid data %s", (value) => {
    expect(readToolUpdatePreference(() => storage(value)).preference).toBe("disabled");
  });

  it("fails closed when the storage getter or getItem throws", () => {
    for (const getStorage of [
      () => { throw new Error("denied"); },
      () => ({ ...storage(), getItem: () => { throw new Error("denied"); } }),
    ]) {
      const result = readToolUpdatePreference(getStorage);
      expect(result.preference).toBe("disabled");
      expect(result.error).toBeTruthy();
    }
  });

  it("reports write failures and refuses an unconfirmed enabled record", () => {
    const saved = storage();
    saved.setItem.mockImplementation(() => { throw new Error("quota"); });
    expect(saveToolUpdatePreference("enabled", () => saved)).toEqual(expect.any(String));
    expect(readToolUpdatePreference(() => saved).preference).toBe("undecided");
    const silentFailure = { ...storage(), setItem: () => {} };
    expect(saveToolUpdatePreference("enabled", () => silentFailure)).toEqual(expect.any(String));
    expect(saveToolUpdatePreference("enabled", () => { throw new Error("denied"); })).toEqual(expect.any(String));
  });
});

it("adopts the installer choice only after confirming its localStorage write", async () => {
  const saved = storage();
  const result = await initializeToolUpdatePreference(() => saved, () => Promise.resolve("enabled"), () => true);
  expect(result).toEqual({ preference: "enabled", error: null });
  expect(saved.getItem(TOOL_UPDATE_PREFERENCE_KEY)).toBe("enabled");
});

describe("installer preference handoff", () => {
  it.each(["enabled", "disabled"] as const)("preserves an existing %s choice without reading installer data", async (preference) => {
    const saved = storage(preference);
    const readInstaller = vi.fn(() => Promise.resolve(preference === "enabled" ? "disabled" as const : "enabled" as const));
    expect(await initializeToolUpdatePreference(() => saved, readInstaller, () => true)).toEqual({ preference, error: null });
    expect(readInstaller).not.toHaveBeenCalled();
    expect(saved.setItem).not.toHaveBeenCalled();
  });

  it("keeps the existing consent flow when the installer file is absent", async () => {
    const saved = storage("undecided");
    expect(await initializeToolUpdatePreference(() => saved, () => Promise.resolve(null), () => true)).toEqual({ preference: "undecided", error: null });
    expect(saved.setItem).not.toHaveBeenCalled();
  });

  it.each(["enabled", "disabled"] as const)("imports %s without reopening consent and permits a later user change", async (preference) => {
    const saved = storage();
    const imported = await initializeToolUpdatePreference(() => saved, () => Promise.resolve(preference), () => true);
    expect(imported).toEqual({ preference, error: null });
    const h = harness(imported?.preference);
    h.connect();
    h.result.resolve(noUpdate);
    await runToolUpdate(h.session, "automatic", h.actions);
    expect(h.install).toHaveBeenCalledTimes(preference === "enabled" ? 1 : 0);
    const next = preference === "enabled" ? "disabled" : "enabled";
    expect(saveToolUpdatePreference(next, () => saved)).toBeNull();
    expect(readToolUpdatePreference(() => saved).preference).toBe(next);
  });

  it.each(["throw", "silent"])("keeps updates off after a %s storage write failure", async (failure) => {
    const saved = storage();
    saved.setItem.mockImplementation(() => { if (failure === "throw") throw new Error("quota"); });
    const imported = await initializeToolUpdatePreference(() => saved, () => Promise.resolve("enabled"), () => true);
    expect(imported?.preference).toBe("disabled");
    expect(imported?.error).toBeTruthy();
    const h = harness(imported?.preference);
    h.connect();
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(h.install).not.toHaveBeenCalled();
  });

  it("fails closed for unreadable installer data or invalid saved data", async () => {
    const saved = storage();
    const result = await initializeToolUpdatePreference(() => saved, () => Promise.reject(new Error("invalid file")), () => true);
    expect(result?.preference).toBe("disabled");
    expect(result?.error).toBeTruthy();
    expect(saved.setItem).not.toHaveBeenCalled();
    const invalid = storage("true");
    const readInstaller = vi.fn(() => Promise.resolve("enabled" as const));
    expect((await initializeToolUpdatePreference(() => invalid, readInstaller, () => true))?.preference).toBe("disabled");
    expect(readInstaller).not.toHaveBeenCalled();
  });

  it("does not apply abandoned effects or overwrite a choice made during loading", async () => {
    const saved = storage();
    const pending = deferred<"enabled">();
    let active = true;
    const abandoned = initializeToolUpdatePreference(() => saved, () => pending.promise, () => active);
    active = false;
    pending.resolve("enabled");
    expect(await abandoned).toBeNull();
    expect(saved.setItem).not.toHaveBeenCalled();
    const second = deferred<"enabled">();
    const operation = initializeToolUpdatePreference(() => saved, () => second.promise, () => true);
    saveToolUpdatePreference("disabled", () => saved);
    second.resolve("enabled");
    expect(await operation).toEqual({ preference: "disabled", error: null });
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function harness(preference: ToolUpdatePreference = "enabled") {
  const session = createToolUpdateSession();
  let view: ToolUpdateView = { ...initialToolUpdateView };
  let currentPreference = preference;
  const result = deferred<ToolUpdateResult>();
  const install = vi.fn<(operationId: string) => Promise<ToolUpdateResult>>(() => result.promise);
  const refreshReport = vi.fn(() => Promise.resolve());
  const actions = {
    preference: () => currentPreference,
    canStart: vi.fn(() => true),
    createId: vi.fn(() => "operation-1"),
    install, refreshReport,
    setView: (update: (previous: ToolUpdateView) => ToolUpdateView) => { view = update(view); },
    errorMessage: () => "Tools could not be updated safely. Existing tools remain available.",
  };
  const connect = (subscribe: (receive: (event: ToolUpdateProgress) => void) => Promise<() => void> = () => Promise.resolve(vi.fn())) =>
    connectToolProgress(session, subscribe, (progress) => { view = { ...view, progress }; });
  return { session, actions, result, connect, install, refreshReport, view: () => view,
    preference: (value: ToolUpdatePreference) => { currentPreference = value; } };
}

const noUpdate: ToolUpdateResult = { updatedTools: [], restartRequired: false };
const updated: ToolUpdateResult = { updatedTools: ["yt-dlp", "ffmpeg"], restartRequired: true };
const progress: ToolUpdateProgress = { operationId: "operation-1", stage: "downloading", toolName: "FFmpeg", downloadedBytes: 4, totalBytes: 10 };

describe("App-owned tool update coordination", () => {
  it.each(["undecided", "disabled"] as const)("does not invoke automatic updates when %s", async (preference) => {
    const h = harness(preference);
    h.connect();
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(h.install).not.toHaveBeenCalled();
    expect(h.session.autoAttempted).toBe(false);
  });

  it("waits for the listener before invoking and stays silent when there is no update", async () => {
    const h = harness();
    const listening = deferred<() => void>();
    const stop = vi.fn();
    h.connect(() => listening.promise);
    const operation = runToolUpdate(h.session, "automatic", h.actions);
    await Promise.resolve();
    expect(h.install).not.toHaveBeenCalled();
    expect(h.session.autoAttempted).toBe(false);
    listening.resolve(stop);
    await h.session.listener?.ready;
    expect(h.install).toHaveBeenCalledExactlyOnceWith("operation-1");
    expect(h.session.autoAttempted).toBe(true);
    h.result.resolve(noUpdate);
    expect(await operation).toBe(true);
    expect(h.view()).toMatchObject({ installing: false, restartRequired: false, notice: null, error: null });
    expect(h.refreshReport).toHaveBeenCalledOnce();
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(h.install).toHaveBeenCalledOnce();
  });

  it("survives StrictMode setup/cleanup/setup without consuming the automatic attempt early", async () => {
    const h = harness();
    const firstReady = deferred<() => void>();
    const secondReady = deferred<() => void>();
    const firstStop = vi.fn();
    const secondStop = vi.fn();
    const cleanup = h.connect(() => firstReady.promise);
    const first = runToolUpdate(h.session, "automatic", h.actions);
    await Promise.resolve();
    cleanup();
    h.connect(() => secondReady.promise);
    const second = runToolUpdate(h.session, "automatic", h.actions);
    firstReady.resolve(firstStop);
    expect(await first).toBe(false);
    expect(h.install).not.toHaveBeenCalled();
    expect(firstStop).toHaveBeenCalledOnce();
    secondReady.resolve(secondStop);
    h.result.resolve(noUpdate);
    await second;
    expect(h.install).toHaveBeenCalledOnce();
    h.session.listener?.dispose();
    expect(secondStop).toHaveBeenCalledOnce();
  });

  it("cleans a late subscription after unmount and never invokes the backend", async () => {
    const h = harness();
    const listening = deferred<() => void>();
    const stop = vi.fn();
    const cleanup = h.connect(() => listening.promise);
    const operation = runToolUpdate(h.session, "automatic", h.actions);
    await Promise.resolve();
    cleanup();
    cleanup();
    listening.resolve(stop);
    expect(await operation).toBe(false);
    expect(stop).toHaveBeenCalledOnce();
    expect(h.install).not.toHaveBeenCalled();
    expect(h.session.autoAttempted).toBe(false);
  });

  it("can reconsider an operation that never started after a subscription failure", async () => {
    const h = harness();
    h.connect(() => Promise.reject(new Error("subscription failed")));
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(h.session.autoAttempted).toBe(false);
    expect(h.view().error).toContain("progress could not be connected");
    h.connect();
    h.result.resolve(noUpdate);
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(true);
    expect(h.install).toHaveBeenCalledOnce();
    expect(h.view().error).toBeNull();
  });

  it("enables once later in the session, permits manual updates, and resets only for a new session", async () => {
    const h = harness("disabled");
    h.connect();
    h.result.resolve(noUpdate);
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    h.preference("enabled");
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(true);
    h.preference("disabled");
    h.preference("enabled");
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(await runToolUpdate(h.session, "manual", h.actions)).toBe(true);
    expect(h.install).toHaveBeenCalledTimes(2);
    const nextLaunch = harness();
    nextLaunch.connect();
    nextLaunch.result.resolve(noUpdate);
    expect(await runToolUpdate(nextLaunch.session, "automatic", nextLaunch.actions)).toBe(true);
  });

  it("does not enable an automatic run after an unsuccessful preference write", async () => {
    const h = harness("undecided");
    h.connect();
    const saved = storage();
    saved.setItem.mockImplementation(() => { throw new Error("quota"); });
    if (!saveToolUpdatePreference("enabled", () => saved)) h.preference("enabled");
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(h.install).not.toHaveBeenCalled();
  });

  it("rechecks permission and startup conditions after awaiting the listener", async () => {
    const h = harness();
    const listening = deferred<() => void>();
    h.connect(() => listening.promise);
    const operation = runToolUpdate(h.session, "automatic", h.actions);
    h.preference("disabled");
    listening.resolve(vi.fn());
    expect(await operation).toBe(false);
    expect(h.install).not.toHaveBeenCalled();
    h.preference("enabled");
    h.actions.canStart.mockReturnValue(false);
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    h.actions.canStart.mockReturnValue(true);
    h.result.resolve(noUpdate);
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(true);
  });

  it("ignores stale operation events and keeps progress/result without a mounted ToolsDialog", async () => {
    const h = harness();
    let receive: (event: ToolUpdateProgress) => void = () => {};
    h.connect((callback) => { receive = callback; return Promise.resolve(vi.fn()); });
    const operation = runToolUpdate(h.session, "automatic", h.actions);
    await h.session.listener?.ready;
    receive({ ...progress, operationId: "old-operation" });
    expect(h.view().progress?.stage).toBe("checking");
    receive(progress);
    expect(h.view().progress).toEqual(progress);
    receive({ ...progress, stage: "installing" });
    expect(h.view().restartRequired).toBe(false);
    expect(h.view().notice).toBeNull();
    h.result.resolve(updated);
    await operation;
    expect(h.view()).toMatchObject({ restartRequired: true, notice: TOOL_UPDATE_RESTART_NOTICE, installing: false });
    const finalState = h.view();
    receive(progress);
    expect(h.view()).toBe(finalState);
  });

  it("reconnects progress during an active operation without installing twice", async () => {
    const h = harness();
    let firstReceive: (event: ToolUpdateProgress) => void = () => {};
    let secondReceive: (event: ToolUpdateProgress) => void = () => {};
    const stop = vi.fn();
    const cleanup = h.connect((receive) => { firstReceive = receive; return Promise.resolve(stop); });
    const operation = runToolUpdate(h.session, "automatic", h.actions);
    await h.session.listener?.ready;
    cleanup();
    h.connect((receive) => { secondReceive = receive; return Promise.resolve(vi.fn()); });
    await h.session.listener?.ready;
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    firstReceive({ ...progress, downloadedBytes: 9 });
    expect(h.view().progress?.stage).toBe("checking");
    secondReceive(progress);
    expect(h.view().progress).toEqual(progress);
    expect(stop).toHaveBeenCalledOnce();
    h.result.resolve(updated);
    await operation;
    expect(h.install).toHaveBeenCalledOnce();
    expect(h.view().restartRequired).toBe(true);
  });

  it("does not start automatic or duplicate manual requests while manual installation is active", async () => {
    const h = harness("disabled");
    h.connect();
    const manual = runToolUpdate(h.session, "manual", h.actions);
    await h.session.listener?.ready;
    h.preference("enabled");
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(await runToolUpdate(h.session, "manual", h.actions)).toBe(false);
    expect(h.session.autoAttempted).toBe(false);
    expect(h.install).toHaveBeenCalledOnce();
    h.result.resolve(noUpdate);
    await manual;
  });

  it("lets the current update finish after opt-out, without another automatic attempt", async () => {
    const h = harness();
    h.connect();
    const operation = runToolUpdate(h.session, "automatic", h.actions);
    await h.session.listener?.ready;
    h.preference("disabled");
    expect(h.session.operationId).toBe("operation-1");
    h.result.resolve(updated);
    await operation;
    expect(h.view().restartRequired).toBe(true);
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
    expect(h.install).toHaveBeenCalledOnce();
  });

  it("silences only automatic structural Conflict errors and does not retry", async () => {
    for (const source of ["automatic", "manual"] as const) {
      const h = harness();
      h.connect();
      h.install.mockRejectedValue({ code: "conflict", message: "different wording" });
      await runToolUpdate(h.session, source, h.actions);
      expect(h.view().error === null).toBe(source === "automatic");
      if (source === "automatic") expect(await runToolUpdate(h.session, source, h.actions)).toBe(false);
      expect(h.install).toHaveBeenCalledOnce();
    }
    for (const error of [new Error("already in progress"), { code: "tool_update_failed" }, null, "conflict"]) expect(isToolUpdateConflict(error)).toBe(false);
  });

  it.each(["network", "json", "sha256", "filesystem"])("preserves safe error feedback for %s failures", async (failure) => {
    const h = harness();
    h.connect();
    h.install.mockRejectedValue({ code: "tool_update_failed", message: failure });
    await runToolUpdate(h.session, "automatic", h.actions);
    expect(h.view()).toMatchObject({ installing: false, restartRequired: false, notice: null });
    expect(h.view().error).toContain("Existing tools remain available");
    expect(h.refreshReport).not.toHaveBeenCalled();
    expect(await runToolUpdate(h.session, "automatic", h.actions)).toBe(false);
  });

  it("preserves restart information when report refresh fails or a later manual check has no update", async () => {
    const h = harness();
    h.connect();
    h.refreshReport.mockRejectedValue(new Error("offline"));
    h.result.resolve(updated);
    await runToolUpdate(h.session, "automatic", h.actions);
    expect(h.view().restartRequired).toBe(true);
    expect(h.view().notice).toBe(TOOL_UPDATE_RESTART_NOTICE);
    expect(h.view().error).toContain("status could not be refreshed");
    h.install.mockResolvedValue(noUpdate);
    h.refreshReport.mockResolvedValue();
    await runToolUpdate(h.session, "manual", h.actions);
    expect(h.view().restartRequired).toBe(true);
    expect(h.view().notice).toBe(TOOL_UPDATE_RESTART_NOTICE);
  });
});
