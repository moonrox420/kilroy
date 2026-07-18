import * as React from "react";
import { cn } from "@/lib/utils";

export const Input = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, type = "text", ...props }, ref) => (
  <input
    ref={ref}
    type={type}
    className={cn(
      "flex h-8 w-full rounded-md border border-line bg-bg-2 px-2 text-[12px] text-ink",
      "placeholder:text-ink-subtle",
      "focus:border-amber focus:outline-none focus:ring-1 focus:ring-amber/40",
      "disabled:opacity-50",
      className,
    )}
    {...props}
  />
));
Input.displayName = "Input";
