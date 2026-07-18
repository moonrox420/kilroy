import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** shadcn helper — merge class names with tailwind-aware conflict resolution. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function basename(path: string): string {
  const norm = path.replace(/\\/g, "/");
  const i = norm.lastIndexOf("/");
  return i === -1 ? norm : norm.slice(i + 1);
}

export function dirname(path: string): string {
  const norm = path.replace(/\\/g, "/");
  const i = norm.lastIndexOf("/");
  return i === -1 ? "" : norm.slice(0, i);
}

export function extname(path: string): string {
  const base = basename(path);
  const i = base.lastIndexOf(".");
  return i === -1 || i === 0 ? "" : base.slice(i).toLowerCase();
}

/** Crude byte → human-readable. */
export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** Map a file extension to a Monaco language id. */
export function languageForPath(path: string): string {
  switch (extname(path)) {
    case ".ts":
    case ".mts":
    case ".cts":
      return "typescript";
    case ".tsx":
      return "typescript";
    case ".js":
    case ".mjs":
    case ".cjs":
      return "javascript";
    case ".jsx":
      return "javascript";
    case ".rs":
      return "rust";
    case ".py":
      return "python";
    case ".go":
      return "go";
    case ".rb":
      return "ruby";
    case ".java":
      return "java";
    case ".kt":
    case ".kts":
      return "kotlin";
    case ".c":
    case ".h":
      return "c";
    case ".cpp":
    case ".cc":
    case ".hpp":
      return "cpp";
    case ".cs":
      return "csharp";
    case ".swift":
      return "swift";
    case ".php":
      return "php";
    case ".lua":
      return "lua";
    case ".sh":
    case ".bash":
    case ".zsh":
      return "shell";
    case ".ps1":
    case ".psm1":
      return "powershell";
    case ".sql":
      return "sql";
    case ".html":
    case ".htm":
      return "html";
    case ".css":
      return "css";
    case ".scss":
    case ".sass":
      return "scss";
    case ".less":
      return "less";
    case ".json":
      return "json";
    case ".yml":
    case ".yaml":
      return "yaml";
    case ".toml":
      return "toml";
    case ".xml":
      return "xml";
    case ".md":
    case ".markdown":
      return "markdown";
    case ".dockerfile":
      return "dockerfile";
    default:
      return "plaintext";
  }
}
