// Formatting rules from the spec: binary units shown as KB/MB/GB with one
// decimal, dates in local time. Mirrors `cleaner_core::format`.

const UNITS = ["KB", "MB", "GB", "TB", "PB", "EB"];

export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1023.95 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${UNITS[unit]}`;
}

export function percent(part: number, whole: number): number {
  return whole > 0 ? (part * 100) / whole : 0;
}

export function formatPercent(part: number, whole: number): string {
  const p = percent(part, whole);
  if (p === 0) return "0%";
  if (p < 0.1) return "<0.1%";
  return `${p.toFixed(1)}%`;
}

export function formatCount(n: number): string {
  return n.toLocaleString();
}

const dateTimeFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

/** Unix seconds to a local date and time. */
export function formatDateTime(unixSeconds: number): string {
  if (!unixSeconds) return "—";
  return dateTimeFormat.format(new Date(unixSeconds * 1000));
}

/** Unix seconds to a local date (for compact columns). */
export function formatDate(unixSeconds: number): string {
  if (!unixSeconds) return "—";
  return dateFormat.format(new Date(unixSeconds * 1000));
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  const m = Math.floor(ms / 60_000);
  const s = Math.round((ms % 60_000) / 1000);
  return `${m} min ${s} s`;
}

export type Category =
  | "folder"
  | "video"
  | "audio"
  | "image"
  | "archive"
  | "program"
  | "diskImage"
  | "document"
  | "code"
  | "other";

const EXTENSIONS: Record<Exclude<Category, "folder" | "other">, string[]> = {
  video: ["mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "flv", "mpg", "mpeg", "ts"],
  audio: ["mp3", "flac", "wav", "aac", "ogg", "m4a", "wma", "opus"],
  image: ["jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "tif", "tiff", "raw", "cr2", "nef", "psd", "svg", "ico"],
  archive: ["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst", "cab", "tgz"],
  program: ["exe", "msi", "msix", "appx", "dll", "sys", "bat", "cmd", "ps1"],
  diskImage: ["iso", "img", "vhd", "vhdx", "vmdk", "qcow2", "vdi", "wim", "esd"],
  document: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "rtf", "odt", "csv", "epub"],
  code: ["js", "mjs", "rs", "py", "java", "c", "cpp", "h", "cs", "go", "json", "xml", "yml", "yaml", "toml", "html", "css"],
};

const BY_EXTENSION = new Map<string, Category>(
  Object.entries(EXTENSIONS).flatMap(([cat, exts]) => exts.map((e) => [e, cat as Category] as const)),
);

/** File type group, used for treemap colors and the legend. */
export function category(name: string, isDir: boolean): Category {
  if (isDir) return "folder";
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "other";
  return BY_EXTENSION.get(name.slice(dot + 1).toLowerCase()) ?? "other";
}

export const CATEGORY_LABELS: Record<Category, string> = {
  folder: "Folders",
  video: "Video",
  audio: "Audio",
  image: "Images",
  archive: "Archives",
  program: "Programs",
  diskImage: "Disk images",
  document: "Documents",
  code: "Code & data",
  other: "Other",
};
