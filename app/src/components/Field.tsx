import type { InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";
import { useId } from "react";

interface LabelProps {
  label: ReactNode;
  hint?: ReactNode;
  className?: string;
}

function Label({ id, label, hint, children, className = "" }: LabelProps & { id: string; children: ReactNode }) {
  return (
    <div className={`flex flex-col gap-1 ${className}`}>
      <label htmlFor={id} className="text-[10px] uppercase tracking-[0.12em] text-muted">{label}</label>
      {children}
      {hint && <span className="text-[10px] text-muted">{hint}</span>}
    </div>
  );
}

export function Field({ label, hint, className, ...rest }: LabelProps & InputHTMLAttributes<HTMLInputElement>) {
  const id = useId();
  return (
    <Label id={id} label={label} hint={hint} className={className}>
      <input id={id} className="field" {...rest} />
    </Label>
  );
}

export function TextArea({ label, hint, className, ...rest }: LabelProps & TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const id = useId();
  return (
    <Label id={id} label={label} hint={hint} className={className}>
      <textarea id={id} className="field resize-y" rows={4} {...rest} />
    </Label>
  );
}

export function Select({ label, hint, className, children, ...rest }: LabelProps & SelectHTMLAttributes<HTMLSelectElement>) {
  const id = useId();
  return (
    <Label id={id} label={label} hint={hint} className={className}>
      <select id={id} className="field" {...rest}>{children}</select>
    </Label>
  );
}
