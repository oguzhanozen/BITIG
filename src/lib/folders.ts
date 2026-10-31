export function folderNameError(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "Enter a folder name.";
  if ([...trimmed].length > 120) return "Enter a folder name of up to 120 characters.";
  return null;
}
