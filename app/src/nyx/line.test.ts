import { describe, expect, it } from "vitest";
import type { AgentSession, Delegation, Snapshot } from "../api/contract";
import { createMockBackend } from "../api/mock";
import { isJokeSeed, nyxLine } from "./line";

const tok = { input: 0, output: 0, cacheWrite: 0, cacheRead: 0 };
const agent = (over: Partial<AgentSession> = {}): AgentSession => ({
  sessionId: "s", agent: "claude-code", cwd: "C:\\x\\projeto demo", folderName: "projeto demo", contextId: null,
  status: "done", startedAt: "2026-11-07T10:00:00Z", updatedAt: "2026-11-07T10:00:00Z", lastMessage: null,
  tokens: tok, costUsd: 0, ...over,
});
const snap = (agents: AgentSession[] = [], percent: number | null = 40): Snapshot => ({
  status: "active", activeDelegations: 0, agents, unreadEvents: 0, settings: {} as never,
  contexts: [{ id: "c1", name: "projeto demo", rootPath: "C:\\x", instructions: "", color: null, createdAt: "", allowedTools: [], references: [], aiEnabled: false }],
  weekUsage: { range: "week", contextId: null, from: "", to: "", tokens: tok, costUsd: 0, sessions: 0, messages: 0, byModel: [], ceilingUsd: 100, percent },
});
const deleg = (status: Delegation["status"], over: Partial<Delegation> = {}): Delegation => ({
  id: "d", contextId: "c1", title: "t", status, currentStep: 1,
  steps: [0, 1, 2, 3].map((index) => ({ index, prompt: "", status: "pending" as const, attempts: 0, report: null, aiSummary: null, feedback: [], commit: null, costUsd: 0, contextFill: null, liveActivity: null, startedAt: null, finishedAt: null })),
  worktreePath: "", branch: "", baseBranch: "main", claudeSessionId: null, handoffs: 0, blockedReason: null, costUsd: 0,
  createdAt: "2026-11-07T10:00:00Z", updatedAt: "2026-11-07T10:00:00Z", ...over,
});

// Find stable timestamps for the seeded branches.
const times = Array.from({ length: 4000 }, (_, i) => new Date(1_700_000_000_000 + i * 1237));
const jokeTime = times.find((t) => isJokeSeed(t))!;
const plainTime = times.find((t) => !isJokeSeed(t))!;
const open = (now = plainTime) => ({ justOpened: true, now });
const idle = { justOpened: false, now: plainTime };

describe("priority order", () => {
  it("checkpoint wins over everything", () => {
    const s = snap([agent({ status: "waiting" }), agent({ status: "error" })], 99);
    expect(nyxLine(s, [deleg("blocked"), deleg("checkpoint")], idle)).toBe("Etapa 2 de 4 concluída. Aprova?");
  });
  it("blocked comes next, naming the context and addressing Dev", () => {
    expect(nyxLine(snap([agent({ status: "waiting" })]), [deleg("blocked")], idle)).toBe("Dev, Projeto demo travou numa permissão.");
  });
  it("waiting session", () => {
    expect(nyxLine(snap([agent({ status: "waiting" }), agent({ status: "error" })]), [], idle)).toBe("Projeto demo quer uma resposta sua.");
  });
  it("error session", () => {
    expect(nyxLine(snap([agent({ status: "error", folderName: null, cwd: "C:\\a\\loja-api" })], 99), [], idle)).toBe("Dev, falhou em Loja-api.");
  });
  it("usage at 95% and above uses Dev", () => {
    expect(nyxLine(snap([agent({ status: "running" })], 96.2), [], idle)).toBe("Dev, semana em 96% do teto.");
  });
  it("usage at 80% and above stays with Dev (no name)", () => {
    expect(nyxLine(snap([], 82), [], idle)).toBe("Semana em 82% do teto.");
    expect(nyxLine(snap([], 79.9), [], idle)).toBe("Tudo quieto.");
  });
  it("running agents, singular and plural", () => {
    expect(nyxLine(snap([agent({ status: "running" })]), [], idle)).toBe("1 agente rodando.");
    expect(nyxLine(snap([agent({ status: "running" }), agent({ status: "running" })]), [], idle)).toBe("2 agentes rodando.");
  });
  it("nothing to report", () => {
    expect(nyxLine(snap([agent()]), [deleg("done")], idle)).toBe("Tudo quieto.");
  });
  it("picks the newest among equals", () => {
    const a = agent({ folderName: "velho", status: "waiting", updatedAt: "2026-11-07T09:00:00Z" });
    const b = agent({ folderName: "novo", status: "waiting", updatedAt: "2026-11-07T11:00:00Z" });
    expect(nyxLine(snap([a, b]), [], idle)).toBe("Novo quer uma resposta sua.");
  });
});

describe("opening prefix", () => {
  it("is absent unless justOpened", () => {
    expect(nyxLine(snap([agent({ status: "waiting" })]), [], idle)).not.toMatch(/Dev|De volta|Online/);
  });
  it("uses De volta, Dev. (or sometimes Online, Dev.) on open", () => {
    const lines = new Set(times.slice(0, 200).map((now) => nyxLine(snap([agent({ status: "waiting" })]), [], open(now)).split(" Projeto")[0]));
    expect(lines).toEqual(new Set(["De volta, Dev.", "Online, Dev."]));
    expect(nyxLine(snap([agent({ status: "waiting" })]), [], open())).toMatch(/^(De volta|Online), Dev\. Projeto demo quer uma resposta sua\.$/);
  });
  it("joins the checkpoint question with a comma to keep two sentences", () => {
    expect(nyxLine(snap(), [deleg("checkpoint")], open())).toMatch(/^(De volta|Online), Dev\. Etapa 2 de 4 concluída, aprova\?$/);
  });
  it("serious items open without the friendly name", () => {
    expect(nyxLine(snap([agent({ status: "error" })]), [], open())).toBe("De volta. Dev, falhou em Projeto demo.");
    expect(nyxLine(snap([], 97), [], open())).toBe("De volta. Dev, semana em 97% do teto.");
  });
});

describe("tone rules", () => {
  it("calm lines carry the friendly Dev prefix", () => {
    const calm = [
      nyxLine(snap([agent({ status: "waiting" })]), [], open()),
      nyxLine(snap(), [deleg("checkpoint")], open()),
      nyxLine(snap([], 85), [], open()),
      nyxLine(snap([agent({ status: "running" })]), [], open()),
      nyxLine(snap(), [], open()),
    ];
    expect(calm.every((l) => /Dev/.test(l))).toBe(true);
  });
  it("stays short, with at most 2 sentences, no exclamation or emoji", () => {
    const longName = "um-nome-de-pasta-extremamente-comprido-mesmo";
    const cases = [
      nyxLine(snap([agent({ status: "waiting", folderName: longName })]), [], open()),
      nyxLine(snap([agent({ status: "error", folderName: longName })]), [], open()),
      nyxLine(snap(), [deleg("blocked")], open()),
      nyxLine(snap(), [deleg("checkpoint")], open()),
      nyxLine(snap(), [], open(jokeTime)),
    ];
    for (const l of cases) {
      expect(l.length).toBeLessThanOrEqual(64);
      expect(l).not.toMatch(/!|\p{Extended_Pictographic}/u);
      expect((l.match(/[.?]/g) ?? []).length).toBeLessThanOrEqual(2 + (l.includes("…") ? 1 : 0));
    }
  });
  it("capitalizes the folder name", () => {
    expect(nyxLine(snap([agent({ status: "waiting", folderName: "projeto demo" })]), [], idle)).toMatch(/^Projeto demo /);
  });
});

describe("dry joke", () => {
  it("replaces Tudo quieto only on openings with a joke seed", () => {
    const line = nyxLine(snap(), [], open(jokeTime));
    expect(line).not.toContain("Tudo quieto");
    expect(line).toMatch(/^(De volta|Online), Dev\. /);
    expect(nyxLine(snap(), [], open(plainTime))).toContain("Tudo quieto.");
    expect(nyxLine(snap(), [], { justOpened: false, now: jokeTime })).toBe("Tudo quieto.");
  });
  it("never appears with error, block, alert or any urgent item", () => {
    for (const s of [snap([agent({ status: "error" })]), snap([agent({ status: "waiting" })]), snap([], 97), snap([agent({ status: "running" })])]) {
      expect(nyxLine(s, [], open(jokeTime))).toMatch(/falhou|resposta|semana|rodando/);
    }
    expect(nyxLine(snap(), [deleg("blocked")], open(jokeTime))).toContain("travou");
  });
  it("happens in about 1 of 10 seeds and is deterministic", () => {
    const hits = times.filter((t) => isJokeSeed(t)).length / times.length;
    expect(hits).toBeGreaterThan(0.06);
    expect(hits).toBeLessThan(0.14);
    expect(nyxLine(snap(), [], open(jokeTime))).toBe(nyxLine(snap(), [], open(jokeTime)));
  });
});

describe("with the mock snapshot", () => {
  it("produces the user's example shape", async () => {
    const m = createMockBackend({ intervalMs: 0, delegationStepMs: 0 });
    const s = await m.invoke("get_snapshot", {});
    const ds = await m.invoke("list_delegations", { contextId: "ctx-demo" });
    m.stop();
    expect(nyxLine(s, ds, open())).toMatch(/^(De volta|Online), Dev\. Projeto demo quer uma resposta sua\.$/);
  });
});
