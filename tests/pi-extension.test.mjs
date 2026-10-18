// The pi extension Orbit loads into its pi sessions (src/claude/pi-orbit.ts),
// run with a fake pi: `node --test tests/pi-extension.test.mjs` (make check
// runs it when node is installed).
import { test } from "node:test";
import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const dir = mkdtempSync(join(tmpdir(), "orbit-pi-"));
const log = join(dir, "log");
const bin = join(dir, "orbit");
// A stand-in for Orbit's `report`: one line per call.
writeFileSync(bin, `#!/bin/sh\necho "$3 $4" >> "$2"\n`);
chmodSync(bin, 0o755);
process.env.ORBIT_STATUS_LOG = log;
process.env.ORBIT_BIN = bin;
process.env.ORBIT_PI_RULES = JSON.stringify({
  allow: ["Bash(git commit *)"],
  deny: ["Bash(git push --force*)", "Bash(gh pr merge*)"],
});

const ext = await import("../src/claude/pi-orbit.ts");
const wait = () => new Promise((r) => setTimeout(r, 200));

test("rules: deny wins, allow passes, edits inside the folder run, the rest asks", () => {
  const rules = ext.parseRules(process.env.ORBIT_PI_RULES);
  const cwd = "/w/DEV-1";
  assert.equal(ext.decide(rules, "bash", { command: "git push --force origin x" }, cwd), "deny");
  assert.equal(ext.decide(rules, "bash", { command: "gh pr merge 3" }, cwd), "deny");
  assert.equal(ext.decide(rules, "bash", { command: "git commit -m x" }, cwd), "allow");
  assert.equal(ext.decide(rules, "bash", { command: "rm -rf /" }, cwd), "ask");
  assert.equal(ext.decide(rules, "read", { path: "/etc/hosts" }, cwd), "allow");
  assert.equal(ext.decide(rules, "edit", { path: "src/a.rs" }, cwd), "allow");
  assert.equal(ext.decide(rules, "write", { path: "../other/x" }, cwd), "ask");
  assert.equal(ext.family("cargo test --all"), "cargo test *");
  assert.deepEqual(ext.parseRules("not json"), { allow: [], deny: [] });
});

test("reports working, needs-approval and turn; a declined call is blocked", async () => {
  const handlers = {};
  ext.default({ on: (name, fn) => (handlers[name] = fn) });
  let idle = false;
  let answer = "Decline";
  const ctx = {
    mode: "tui",
    cwd: "/w/DEV-1",
    isIdle: () => idle,
    ui: { select: async () => answer },
  };
  handlers.agent_start({}, ctx);
  await wait();
  const denied = await handlers.tool_call({ toolName: "bash", input: { command: "git push --force" } }, ctx);
  assert.equal(denied.block, true);
  const declined = await handlers.tool_call({ toolName: "bash", input: { command: "make deploy" } }, ctx);
  assert.equal(declined.block, true);
  answer = "Allow `npm test *` for this session";
  assert.equal(await handlers.tool_call({ toolName: "bash", input: { command: "npm test --watch" } }, ctx), undefined);
  // Remembered for the session: no question the second time.
  answer = "Decline";
  assert.equal(await handlers.tool_call({ toolName: "bash", input: { command: "npm test x" } }, ctx), undefined);
  idle = true;
  handlers.agent_settled({}, ctx);
  await wait();
  const lines = readFileSync(log, "utf8").trim().split("\n");
  assert.deepEqual(lines.map((l) => l.split(" ")[0]), [
    "working",
    "needs-approval",
    "working",
    "needs-approval",
    "working",
    "turn",
  ]);
});

test("does nothing outside pi's TUI", async () => {
  const handlers = {};
  ext.default({ on: (name, fn) => (handlers[name] = fn) });
  const before = readFileSync(log, "utf8");
  const ctx = { mode: "rpc", cwd: "/w", isIdle: () => true, ui: {} };
  handlers.agent_start({}, ctx);
  assert.equal(await handlers.tool_call({ toolName: "bash", input: { command: "rm -rf /" } }, ctx), undefined);
  await wait();
  assert.equal(readFileSync(log, "utf8"), before);
});
