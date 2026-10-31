import type { Folder } from "../types/domain";

export function buildFolderOptions(folders: readonly Folder[]): { value: string; label: string }[] {
  const folderIds = new Set(folders.map((folder) => folder.id));
  const children = new Map<string | null, Folder[]>();
  for (const folder of folders) {
    const siblings = children.get(folder.parentId) ?? [];
    siblings.push(folder);
    children.set(folder.parentId, siblings);
  }

  const options: { value: string; label: string }[] = [];
  const visited = new Set<string>();
  function visit(start: Folder) {
    const stack = [{ folder: start, label: start.name }];
    while (stack.length) {
      const { folder, label } = stack.pop()!;
      if (visited.has(folder.id)) continue;
      visited.add(folder.id);
      options.push({ value: folder.id, label });
      for (const child of [...(children.get(folder.id) ?? [])].reverse()) {
        stack.push({ folder: child, label: `${label} / ${child.name}` });
      }
    }
  }

  for (const folder of folders) {
    if (folder.parentId === null || !folderIds.has(folder.parentId)) visit(folder);
  }
  // Keep cyclic folders selectable without visiting any ID twice.
  for (const folder of folders) visit(folder);
  return options;
}

export function formatDuration(durationMs: number | null): string {
  if (durationMs === null || !Number.isFinite(durationMs) || durationMs < 0) return "—";
  const totalSeconds = Math.floor(durationMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
    : `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

const WINDOWS_RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i;

export function sanitizeExportName(title: string): string {
  let value = title
    .replace(/[<>:"/\\|?*]/g, "_")
    .split("")
    .map((character) => character.charCodeAt(0) < 32 ? "_" : character)
    .join("")
    .replace(/[. ]+$/g, "")
    .trim();
  if (!value) value = "media";
  if (WINDOWS_RESERVED.test(value)) value = `_${value}`;
  return [...value].slice(0, 180).join("");
}
