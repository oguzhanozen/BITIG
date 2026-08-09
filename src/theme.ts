export const palettes = [
  { id: "stormy-morning", name: "Stormy morning", colors: ["#6A89A7", "#BDDDFC", "#88BDF2", "#384959"] },
  { id: "blue-eclipse", name: "Blue eclipse", colors: ["#272757", "#8686AC", "#505081", "#0F0E47"] },
  { id: "cotton-candy", name: "Cotton candy skies", colors: ["#B298E7", "#B8E3E9", "#F5B8D5", "#F9BEDD"] },
  { id: "salt-pepper", name: "Salt and pepper", colors: ["#FFFFFF", "#D4D4D4", "#B3B3B3", "#2B2B2B"] },
  { id: "moonlight", name: "Under the moonlight", colors: ["#CCCCFF", "#A3A3CC", "#5C5C99", "#292966"] },
  { id: "emerald-odyssey", name: "Emerald odyssey", colors: ["#00674F", "#73E6CB", "#3EBB9E", "#0A3C30"] },
  { id: "mountain-mist", name: "Mountain mist", colors: ["#6D8196", "#B0C4DE", "#01796F", "#5A5A5A"] },
] as const;

export type PaletteId = (typeof palettes)[number]["id"];
export type ThemeMode = "light" | "dark";

export function isPaletteId(value: string | null): value is PaletteId {
  return palettes.some((palette) => palette.id === value);
}
