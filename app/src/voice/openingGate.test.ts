import { describe, expect, it } from "vitest";
import { createOpeningGate } from "./openingGate";

describe("opening gate", () => {
  it("waits for text, speaks once, ignores re-renders", () => {
    const g = createOpeningGate();
    expect(g.poll(null)).toBeNull();
    expect(g.poll("Oi.")).toBe("Oi.");
    expect(g.poll("Oi.")).toBeNull();
    expect(g.poll("Outra.")).toBeNull();
  });
  it("speaks again only after hidden then visible", () => {
    const g = createOpeningGate();
    g.poll("a");
    g.show();
    expect(g.poll("b")).toBeNull();
    g.hide();
    expect(g.poll("b")).toBeNull();
    g.show();
    expect(g.poll("c")).toBe("c");
    expect(g.poll("c")).toBeNull();
  });
});
