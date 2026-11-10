import { useId, type CSSProperties, type ReactNode } from "react";

export interface SliderProps {
  label: ReactNode;
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  /** Text for the value readout (mono, right of the label). Default: the number. */
  format?: (value: number) => string;
  hint?: ReactNode;
  disabled?: boolean;
  className?: string;
}

/**
 * Range control in HUD style: a segmented track (20 cells, like the usage bar) filled in primary up to the value,
 * a neon square thumb and the value in mono. The native <input type="range"> stays underneath for keyboard,
 * focus and screen readers; only its look is replaced.
 */
export function Slider({ label, value, min, max, step = 1, onChange, format, hint, disabled, className = "" }: SliderProps) {
  const id = useId();
  const span = max - min || 1;
  const k = Math.min(1, Math.max(0, (value - min) / span));
  const text = format ? format(value) : String(value);
  return (
    <div className={`flex flex-col gap-1 ${className}`}>
      <div className="flex items-baseline justify-between gap-2">
        <label htmlFor={id} className="text-[10px] uppercase tracking-[0.12em] text-muted">{label}</label>
        <output htmlFor={id} className="text-[11px] tabular-nums text-text" aria-live="off">{text}</output>
      </div>
      <div className={`slider ${disabled ? "is-disabled" : ""}`} style={{ "--k": k } as CSSProperties}>
        <span className="track" aria-hidden="true"><i className="fill" /></span>
        <input
          id={id}
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          disabled={disabled}
          aria-valuetext={text}
          onChange={(e) => onChange(Number(e.target.value))}
        />
      </div>
      {hint && <span className="text-[10px] text-muted">{hint}</span>}
    </div>
  );
}
