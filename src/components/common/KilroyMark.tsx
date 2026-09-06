/**
 * Kilroy mark — the classic "Kilroy was here" silhouette.
 *
 * A wall, a domed head peeking over it, two small eyes, the iconic long
 * pear-shaped nose drooping well below the wall, and two hands gripping
 * the top of the wall with four visible finger-bumps each. Single-weight
 * strokes so the mark reads at every size from 16px (tray icon) up to
 * 132px (editor watermark) and at every opacity.
 *
 * Proportions match the reference sticker: the nose is the dominant
 * vertical element (extends roughly 14 units below the wall in a 64-unit
 * viewBox, vs. ~12 units of head above the wall). Eyes sit close
 * together in the upper third of the dome. Hands span roughly 1/3 of the
 * wall on each side.
 *
 * Rendered in currentColor so theme tokens flow through (defaults to
 * --amber via the `text-amber` class).
 */
import * as React from "react";
import { cn } from "@/lib/utils";

interface Props extends React.SVGProps<SVGSVGElement> {
  size?: number;
}

export const KilroyMark = React.forwardRef<SVGSVGElement, Props>(
  ({ size = 24, className, ...rest }, ref) => (
    <svg
      ref={ref}
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={cn("text-amber", className)}
      {...rest}
      aria-hidden
    >
      {/* Wall — one clean horizontal line, slightly inset from the edges. */}
      <line
        x1="3"
        y1="36"
        x2="61"
        y2="36"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
      />

      {/* Head dome — smooth rounded U opening downward over the wall.
          Cubic-bezier with control points pulled to make a near-circular
          arc rather than a peaked dome. */}
      <path
        d="M 22 36 C 22 22, 24 12, 32 12 C 40 12, 42 22, 42 36"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        fill="none"
      />

      {/* Eyes — two small filled dots, close together in the upper third
          of the dome. */}
      <circle cx="29" cy="22" r="1.8" fill="currentColor" />
      <circle cx="35" cy="22" r="1.8" fill="currentColor" />

      {/* The iconic long nose — a closed pear-shape that starts narrow
          between the eyes (inside the dome) and droops well below the
          wall line. Widest at the bottom; rounded tip. The Z closes
          back to the start across the top, which is hidden inside the
          dome stroke. */}
      <path
        d="M 31 24
           C 29 28, 29 38, 29 44
           C 29 50, 35 50, 35 44
           C 35 38, 35 28, 33 24 Z"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinejoin="round"
        strokeLinecap="round"
        fill="none"
      />

      {/* Left hand — four finger-bumps gripping the top of the wall.
          Each Q segment is one finger: peak at y=32 (4 units above the
          wall at y=36), valley back down to the wall between fingers. */}
      <path
        d="M 6 36
           Q 8 32 10 36
           Q 12 32 14 36
           Q 16 32 18 36
           Q 20 32 22 36"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />

      {/* Right hand — mirror of the left. */}
      <path
        d="M 42 36
           Q 44 32 46 36
           Q 48 32 50 36
           Q 52 32 54 36
           Q 56 32 58 36"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  ),
);
KilroyMark.displayName = "KilroyMark";
