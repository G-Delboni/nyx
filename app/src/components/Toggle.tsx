import type { ReactNode } from "react";

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  hint?: ReactNode;
  disabled?: boolean;
}

/** Switch (role=switch) in HUD style: a small square slides to the neon side. */
export function Toggle({ checked, onChange, label, hint, disabled }: ToggleProps) {
  return (
    <div className="flex flex-col gap-0.5">
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        className="toggle text-left"
        onClick={() => onChange(!checked)}
      >
        <span className="track" aria-hidden="true" />
        <span>{label}</span>
      </button>
      {hint && <span className="pl-9 text-[10px] text-muted">{hint}</span>}
    </div>
  );
}
