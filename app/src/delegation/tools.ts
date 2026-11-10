// allowedTools presets and helpers (claude --allowedTools entries).
export interface ToolPreset {
  id: "readonly" | "edit" | "edit-tests";
  label: string;
  tools: string[];
}

const READ = ["Read", "Grep", "Glob"];
const EDIT = [...READ, "Edit", "Write"];

export const TOOL_PRESETS: ToolPreset[] = [
  { id: "readonly", label: "Só leitura", tools: READ },
  { id: "edit", label: "Editar arquivos", tools: EDIT },
  { id: "edit-tests", label: "Editar + testes npm", tools: [...EDIT, "Bash(npm test:*)", "Bash(npm run build:*)"] },
];

const key = (tools: string[]) => [...new Set(tools)].sort().join("\n");

/** The preset whose tools match the list exactly (order-insensitive), or null for a custom list. */
export function detectPreset(tools: string[]): ToolPreset | null {
  const k = key(tools);
  return TOOL_PRESETS.find((p) => key(p.tools) === k) ?? null;
}

export function applyPreset(id: ToolPreset["id"]): string[] {
  return [...(TOOL_PRESETS.find((p) => p.id === id)?.tools ?? READ)];
}

/** One tool per line or comma separated -> trimmed, deduped list. Commas inside parentheses are kept. */
export function parseTools(text: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let cur = "";
  const flush = () => {
    const t = cur.trim();
    if (t && !out.includes(t)) out.push(t);
    cur = "";
  };
  for (const ch of text) {
    if (ch === "(") depth++;
    if (ch === ")") depth = Math.max(0, depth - 1);
    if ((ch === "," && depth === 0) || ch === "\n" || ch === "\r") flush();
    else cur += ch;
  }
  flush();
  return out;
}

export function formatTools(tools: string[]): string {
  return tools.join("\n");
}

/** Problems with a single entry: "Name" or "Name(specifier)" with balanced parentheses. */
export function invalidTools(tools: string[]): string[] {
  return tools.filter((t) => !/^[A-Za-z][A-Za-z0-9_]*(\(.+\))?$/.test(t));
}

/** True when the tool would let the agent run arbitrary shell commands. */
export function isBroadBash(tool: string): boolean {
  return tool === "Bash" || tool === "Bash(*)" || tool === "Bash(*:*)";
}

/** One entry per line -> trimmed, deduped, empty lines dropped (references, notes). */
export function parseLines(text: string): string[] {
  return [...new Set(text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean))];
}

export function formatLines(lines: string[]): string {
  return lines.join("\n");
}

/** Best-effort tool name from a block reason, e.g. "precisa de Bash(git push:*), que ..." -> "Bash(git push:*)". */
export function extractBlockedTool(reason: string | null | undefined): string | null {
  if (!reason) return null;
  const spec = /\b([A-Z][A-Za-z]+\([^)]+\))/.exec(reason);
  if (spec) return spec[1];
  const bare = /\b(Bash|Edit|Write|Read|Grep|Glob|WebFetch|WebSearch|NotebookEdit|Task)\b/.exec(reason);
  return bare ? bare[1] : null;
}

/** Context tools with `tool` added once (returns the same array when already present). */
export function withTool(tools: string[], tool: string): string[] {
  const t = tool.trim();
  return !t || tools.includes(t) ? tools : [...tools, t];
}
