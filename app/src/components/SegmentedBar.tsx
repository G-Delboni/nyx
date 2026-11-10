import { segments } from "../lib/format";

export interface SegmentedBarProps {
  /** 0..100+ (null = empty). */
  percent: number | null | undefined;
  label: string;
  /** Dashed cells at these percentages (e.g. alert thresholds [80, 95]). */
  marks?: number[];
  /** Cells at/after this percentage turn neon (default: first mark, or 80). */
  hotFrom?: number;
  cells?: number;
}

/** 20-cell meter with dashed alert marks; filled cells past the first mark glow neon. */
export function SegmentedBar({ percent, label, marks = [], hotFrom, cells = 20 }: SegmentedBarProps) {
  const on = segments(percent, cells);
  const hot = hotFrom ?? marks[0] ?? 80;
  const markCells = new Set(marks.map((m) => Math.min(cells - 1, Math.round((m / 100) * cells) - 1)));
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.min(100, Math.round(percent ?? 0))}
      className="seg"
      style={cells !== 20 ? { gridTemplateColumns: `repeat(${cells}, 1fr)` } : undefined}
    >
      {Array.from({ length: cells }, (_, i) => {
        const filled = i < on;
        const isHot = filled && ((i + 1) / cells) * 100 > hot;
        const isMark = !filled && markCells.has(i);
        return <i key={i} className={filled ? (isHot ? "on hot" : "on") : isMark ? "mark" : ""} />;
      })}
    </div>
  );
}
