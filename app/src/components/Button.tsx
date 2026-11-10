import type { ButtonHTMLAttributes } from "react";

export type ButtonVariant = "primary" | "outline" | "ghost" | "danger";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** primary = neon fill (one per view); outline = violet frame; ghost = quiet; danger = violet, never red. */
  variant?: ButtonVariant;
  size?: "sm" | "md";
}

export function Button({ variant = "outline", size = "md", className = "", type = "button", ...rest }: ButtonProps) {
  const v = variant === "outline" ? "" : variant;
  return <button type={type} className={`btn ${v} ${size === "sm" ? "sm" : ""} ${className}`} {...rest} />;
}
