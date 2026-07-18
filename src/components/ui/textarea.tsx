import * as React from "react";
import { cn } from "@/lib/utils";

export const Textarea = React.forwardRef<
  HTMLTextAreaElement,
  React.TextareaHTMLAttributes<HTMLTextAreaElement>
>(({ className, ...props }, ref) => (
  <textarea
    ref={ref}
    className={cn(
      "flex min-h-[80px] w-full rounded-md border border-line bg-bg-2 p-2 text-[12px] text-ink",
      "placeholder:text-ink-subtle",
      "focus:border-amber focus:outline-none focus:ring-1 focus:ring-amber/40",
      "resize-y disabled:opacity-50",
      className,
    )}
    {...props}
  />
));
Textarea.displayName = "Textarea";
