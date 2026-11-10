import { describe, expect, it } from "vitest";
import { createMockBackend } from "./mock";

describe("mock hooks", () => {
  it("starts not installed, previews without writing, then installs and removes", async () => {
    const m = createMockBackend({ intervalMs: 0 });
    const st = await m.invoke("get_hooks_status", {});
    expect(st.installed).toBe(false);
    expect(st.otherHooks).toBe(1);
    const dry = await m.invoke("install_hooks", { dryRun: true });
    expect(dry.changed).toBe(true);
    expect(dry.backupPath).toBeNull();
    expect((await m.invoke("get_hooks_status", {})).installed).toBe(false);
    const real = await m.invoke("install_hooks", { dryRun: false });
    expect(real.status.installed).toBe(true);
    expect(real.backupPath).not.toBeNull();
    const rm = await m.invoke("uninstall_hooks", { dryRun: false });
    expect(rm.status.installed).toBe(false);
    expect(rm.status.otherHooks).toBe(1);
    m.stop();
  });
});
