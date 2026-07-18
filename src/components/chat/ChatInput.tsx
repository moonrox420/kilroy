/**
 * Chat input — hard-capped height. Stacks vertically as the user types
 * up to maxRows (5), then scrolls internally. The CHAT PANEL itself
 * never grows; per the blueprint, the window must not grow.
 *
 * Multi-modal: paste an image with Ctrl+V or drop one onto the input
 * area. The image gets resized client-side to MAX_DIM (1024 px on the
 * long edge) so a 4K screenshot doesn't push a 30 MB base64 into the
 * Ollama prompt — vision models read perfectly well from 1024 px, and
 * the daemon stays responsive. Up to MAX_IMAGES attachments per turn.
 *
 * The image bytes never round-trip through a sandbox boundary: we
 * read them via the browser FileReader, draw into an OffscreenCanvas /
 * <canvas>, and toDataURL → strip the `data:image/...;base64,` prefix
 * (Ollama wants raw b64). On submit the strings flow straight to the
 * Tauri command as part of the SendMessagePayload.
 */
import { useState, useRef, useEffect, type KeyboardEvent, type ClipboardEvent, type DragEvent } from "react";
import { Send, Square, ImagePlus, X } from "lucide-react";
import { useAgent } from "@/store/agent";
import { Button } from "@/components/ui/button";
import { notify } from "@/store/notifications";
import { useModifierKey } from "@/store/platform";
import { cn } from "@/lib/utils";

const MAX_ROWS = 5;
const LINE_HEIGHT = 18; // px — matches text-[12px] leading-snug
const MAX_IMAGES = 4;
/** Long-edge cap. 1024 is the sweet spot most vision-tuned LLMs were
 *  pretrained around; larger inputs are silently downscaled internally
 *  by the model and just waste tokens on the way in. */
const MAX_DIM = 1024;
/** Hard cap on individual file size BEFORE downscale. 10 MB catches
 *  the "user pasted a raw camera RAW" footgun without blocking normal
 *  screenshots. */
const MAX_FILE_BYTES = 10 * 1024 * 1024;

interface AttachedImage {
  /** Stable id so React's keyed list stays sane across re-renders. */
  id: string;
  /** Raw base64 payload (no `data:` prefix). What Ollama wants. */
  base64: string;
  /** Data-URL form for the local <img> preview only. Not sent. */
  previewUrl: string;
  /** Roughly post-downscale size, for the "1.4 MB" hint chip. */
  approxBytes: number;
}

export function ChatInput() {
  const [value, setValue] = useState("");
  const [images, setImages] = useState<AttachedImage[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const send = useAgent((s) => s.send);
  const isThinking = useAgent((s) => s.isThinking);
  const modKey = useModifierKey();
  const taRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Autogrow up to MAX_ROWS, then internal scroll.
  useEffect(() => {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = `${Math.min(ta.scrollHeight, LINE_HEIGHT * MAX_ROWS + 16)}px`;
  }, [value]);

  const addImageFiles = async (files: File[]) => {
    if (images.length >= MAX_IMAGES) {
      notify.warn(
        "Image limit reached",
        `Up to ${MAX_IMAGES} attachments per message. Remove one to add another.`,
      );
      return;
    }
    const slots = MAX_IMAGES - images.length;
    const accepted: AttachedImage[] = [];
    for (const f of files.slice(0, slots)) {
      if (!f.type.startsWith("image/")) {
        notify.warn("Not an image", `${f.name || "(unnamed)"} skipped — only images are attached.`);
        continue;
      }
      if (f.size > MAX_FILE_BYTES) {
        notify.warn(
          "Image too large",
          `${f.name || "(unnamed)"} is over 10 MB — try exporting a JPEG or PNG screenshot.`,
        );
        continue;
      }
      try {
        const processed = await loadAndDownscale(f);
        accepted.push(processed);
      } catch (err) {
        notify.error("Image decode failed", String(err));
      }
    }
    if (accepted.length > 0) {
      setImages((prev) => [...prev, ...accepted]);
    }
  };

  const onPaste = async (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    const files: File[] = [];
    for (let i = 0; i < items.length; i++) {
      const it = items[i];
      if (it.kind === "file" && it.type.startsWith("image/")) {
        const f = it.getAsFile();
        if (f) files.push(f);
      }
    }
    if (files.length > 0) {
      // Eat the paste so we don't ALSO get the image's name as text in
      // the textarea (some clipboard sources push both).
      e.preventDefault();
      await addImageFiles(files);
    }
  };

  const onDrop = async (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragOver(false);
    const files = Array.from(e.dataTransfer?.files ?? []);
    if (files.length > 0) {
      await addImageFiles(files);
    }
  };

  const onDragOver = (e: DragEvent<HTMLDivElement>) => {
    // Only react when the drag carries actual files (Chrome shows file
    // drags via `types.includes('Files')`).
    if (e.dataTransfer?.types.includes("Files")) {
      e.preventDefault();
      setDragOver(true);
    }
  };

  const removeImage = (id: string) =>
    setImages((prev) => prev.filter((p) => p.id !== id));

  const submit = async () => {
    if ((!value.trim() && images.length === 0) || isThinking) return;
    const next = value;
    const imgs = images.map((i) => i.base64);
    setValue("");
    setImages([]);
    await send(next, imgs.length > 0 ? imgs : undefined);
  };

  const onKey = async (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      await submit();
    }
  };

  return (
    <div className="border-t border-line bg-bg-1 px-2 py-2">
      {/* Attached-image strip. Only renders when there's anything to show. */}
      {images.length > 0 && (
        <div className="mb-2 flex flex-wrap gap-2">
          {images.map((img) => (
            <div
              key={img.id}
              className="relative h-16 w-16 overflow-hidden rounded-md border border-line bg-bg-2"
            >
              <img
                src={img.previewUrl}
                alt="attachment preview"
                className="h-full w-full object-cover"
              />
              <button
                type="button"
                onClick={() => removeImage(img.id)}
                className="absolute right-0 top-0 flex h-4 w-4 items-center justify-center rounded-bl-md bg-bg-0/80 text-ink hover:bg-err hover:text-ink"
                aria-label="Remove image"
                title="Remove"
              >
                <X className="h-3 w-3" />
              </button>
              <span className="absolute bottom-0 left-0 right-0 bg-bg-0/80 px-1 text-[9px] text-ink-subtle">
                {(img.approxBytes / 1024).toFixed(0)} KB
              </span>
            </div>
          ))}
        </div>
      )}

      <div
        onDrop={onDrop}
        onDragOver={onDragOver}
        onDragLeave={() => setDragOver(false)}
        className={cn(
          "flex items-end gap-2 rounded-md border bg-bg-2 px-2 py-1.5 transition-colors",
          dragOver
            ? "border-amber border-dashed bg-amber/5"
            : "border-line focus-within:border-amber focus-within:ring-amber-glow",
        )}
      >
        <Button
          size="icon"
          variant="ghost"
          onClick={() => fileInputRef.current?.click()}
          disabled={isThinking || images.length >= MAX_IMAGES}
          title="Attach image (or paste with Ctrl+V, or drag-drop)"
          className="shrink-0"
        >
          <ImagePlus className="h-3.5 w-3.5" />
        </Button>
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          multiple
          className="hidden"
          onChange={async (e) => {
            const files = Array.from(e.target.files ?? []);
            if (files.length > 0) await addImageFiles(files);
            // Reset so the same file can be re-picked if the user
            // removes it and wants to add it again.
            e.target.value = "";
          }}
        />
        <textarea
          ref={taRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={onKey}
          onPaste={onPaste}
          rows={1}
          placeholder={
            isThinking
              ? "Kilroy is working…"
              : images.length > 0
                ? "Describe what you want done with the attached image(s)…"
                : "Ask Kilroy. Shift+Enter for newline. Paste / drop images for vision models."
          }
          disabled={isThinking}
          className={cn(
            "flex-1 resize-none bg-transparent text-[12px] leading-snug text-ink outline-none placeholder:text-ink-subtle",
            "disabled:opacity-60",
          )}
          style={{
            maxHeight: LINE_HEIGHT * MAX_ROWS + 16,
            minHeight: LINE_HEIGHT,
          }}
        />
        <Button
          size="icon"
          variant={value.trim() || images.length > 0 ? "default" : "ghost"}
          onClick={submit}
          disabled={(!value.trim() && images.length === 0) || isThinking}
          title="Send"
        >
          {isThinking ? (
            <Square className="h-3.5 w-3.5" />
          ) : (
            <Send className="h-3.5 w-3.5" />
          )}
        </Button>
      </div>
      <p className="mt-1 px-1 text-[10px] text-ink-ghost">
        Enter to send · Shift+Enter for newline · {modKey}+V or drop images for vision
        models (LLaVA, bakllava, llama3.2-vision)
      </p>
    </div>
  );
}

/** Read a File, downscale to MAX_DIM on the long edge, and produce
 *  both a raw-base64 payload (for Ollama) and a data-URL preview (for
 *  the local <img>). We use JPEG quality 0.85 because PNGs of
 *  screenshots are huge and the lossy artifacts at 0.85 are invisible
 *  to vision models. */
async function loadAndDownscale(file: File): Promise<AttachedImage> {
  const url = URL.createObjectURL(file);
  try {
    const img = await loadImage(url);
    const { width, height } = scaleTo(img.width, img.height, MAX_DIM);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas 2d context unavailable");
    ctx.drawImage(img, 0, 0, width, height);
    // Re-encode as JPEG to keep the b64 payload small. PNGs are
    // unavoidable for transparency cases but for screenshots JPEG@0.85
    // is dramatically smaller and indistinguishable to a vision model.
    const dataUrl = canvas.toDataURL("image/jpeg", 0.85);
    const base64 = dataUrl.replace(/^data:image\/\w+;base64,/, "");
    // Rough byte estimate — base64 is ~4/3 the underlying binary size.
    const approxBytes = Math.floor((base64.length * 3) / 4);
    return {
      id: crypto.randomUUID(),
      base64,
      previewUrl: dataUrl,
      approxBytes,
    };
  } finally {
    URL.revokeObjectURL(url);
  }
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("decode failed"));
    img.src = src;
  });
}

function scaleTo(w: number, h: number, maxDim: number) {
  if (w <= maxDim && h <= maxDim) return { width: w, height: h };
  const ratio = w >= h ? maxDim / w : maxDim / h;
  return { width: Math.round(w * ratio), height: Math.round(h * ratio) };
}
