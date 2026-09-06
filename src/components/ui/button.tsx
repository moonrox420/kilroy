import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-md text-[12px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        default:
          "bg-amber text-amber-ink hover:bg-amber-glow active:bg-amber-glow",
        secondary:
          "bg-bg-2 text-ink hover:bg-bg-3 border border-line",
        ghost:
          "text-ink-muted hover:bg-bg-2 hover:text-ink",
        outline:
          "border border-line bg-transparent text-ink hover:bg-bg-2",
        link: "text-amber underline-offset-4 hover:underline",
        destructive: "bg-err text-ink hover:opacity-90",
      },
      size: {
        default: "h-7 px-3",
        sm: "h-6 px-2 text-[11px]",
        lg: "h-9 px-4 text-[13px]",
        icon: "h-7 w-7",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        ref={ref}
        className={cn(buttonVariants({ variant, size, className }))}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { buttonVariants };
