// Builds nyx-hook and places it where Tauri expects a sidecar (externalBin):
// src-tauri/binaries/nyx-hook-<target-triple>.exe. Installed next to nyx.exe as nyx-hook.exe.
// Owner: orchestrator.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const release = process.argv.includes("--release");
const triple = execFileSync("rustc", ["--print", "host-tuple"], { encoding: "utf8" }).trim();
const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");

execFileSync("cargo", ["build", "-p", "nyx-hook", ...(release ? ["--release"] : [])], {
  cwd: root,
  stdio: "inherit",
});

const src = join(targetDir, release ? "release" : "debug", "nyx-hook.exe");
const outDir = join(root, "app", "src-tauri", "binaries");
mkdirSync(outDir, { recursive: true });
copyFileSync(src, join(outDir, `nyx-hook-${triple}.exe`));
console.log(`sidecar: nyx-hook-${triple}.exe`);
