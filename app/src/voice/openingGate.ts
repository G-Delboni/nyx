// Decides when the spoken opening fires: once per panel opening (first time the text is
// ready, then again each time the window turns visible after having been hidden).
export interface OpeningGate {
  /** Window became hidden: the next visible counts as a new opening. */
  hide(): void;
  /** Window became visible. */
  show(): void;
  /** Call on every render/update with the current text (null = not ready). Returns the text to speak, or null. */
  poll(text: string | null): string | null;
}

export function createOpeningGate(): OpeningGate {
  let pending = true;
  let hidden = false;
  return {
    hide() { hidden = true; },
    show() { if (hidden) { hidden = false; pending = true; } },
    poll(text) {
      if (!pending || !text) return null;
      pending = false;
      return text;
    },
  };
}
