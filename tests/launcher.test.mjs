import { test, expect } from "bun:test";
import { join } from "node:path";

const launcher = join(import.meta.dir, "../bin/agent-experiments.mjs");
test("Bun wrapper invokes the native public command", () => {
  const result = Bun.spawnSync([process.execPath, launcher, "--version"]);
  expect(result.exitCode).toBe(0);
  expect(result.stdout.toString()).toBe("agent-experiments 0.0.1\n");
});
test("Bun wrapper preserves native argument failures", () => {
  const result = Bun.spawnSync([process.execPath, launcher, "--unknown-552-option"]);
  expect(result.exitCode).toBe(2);
  expect(result.stderr.toString()).toContain("unexpected argument");
});

test("missing optional native package is an explicit failure without fallback", async () => {
  const { mkdtemp, copyFile, rm } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const directory = await mkdtemp(join(tmpdir(), "launcher-552-"));
  try {
    const isolated = join(directory, "launcher.mjs");
    await copyFile(launcher, isolated);
    const result = Bun.spawnSync([process.execPath, isolated, "--version"]);
    expect(result.exitCode).toBe(1);
    expect(result.stderr.toString()).toContain("Missing @sebastienelet/");
    expect(result.stdout.toString()).toBe("");
  } finally {
    await rm(directory, { recursive: true });
  }
});
