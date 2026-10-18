// Installed by Orbit for the pi sessions it starts (`pi -e <this file>`);
// Orbit rewrites it on every start, so edits here do not last.
// ORBIT_PI_VERSION=1
//
// What it does, only in pi's interactive TUI and only when Orbit started
// the session (ORBIT_STATUS_LOG and ORBIT_BIN are set):
// - reports the session's state to Orbit's status log through Orbit's own
//   `report` command, the way Claude Code's hooks do: working when a run
//   starts, turn when pi is idle again, needs-approval while it asks;
// - applies Orbit's rules for this session (ORBIT_PI_RULES, Claude Code's
//   format): a denied bash command is blocked; an allowed one, a read-only
//   tool, or an edit inside the session's folder runs; anything else asks in
//   pi's TUI first.
// @ts-nocheck

import { execFile } from "node:child_process";
import path from "node:path";

const log = process.env.ORBIT_STATUS_LOG;
const bin = process.env.ORBIT_BIN;

function enabled(ctx) {
  return !!log && !!bin && ctx?.mode === "tui";
}

// One at a time, in order: the log's last line is what Orbit shows.
let reports = Promise.resolve();
function report(state, text = "") {
  reports = reports.then(
    () => new Promise((done) => execFile(bin, ["report", log, state, text], () => done())),
  );
}

// `*` matches anything, the rest literally, the whole string.
function glob(pattern) {
  const body = pattern.split("*").map((p) => p.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*");
  return new RegExp(`^${body}$`, "s");
}

// "Bash(git push *)" -> ["bash", /^git push .*$/]; Edit/Write -> path globs.
// Rules in other forms are not understood and match nothing (so they ask).
export function parseRules(json) {
  const rules = { allow: [], deny: [] };
  let given = {};
  try {
    given = JSON.parse(json || "{}");
  } catch {
    return rules;
  }
  for (const kind of ["allow", "deny"]) {
    for (const rule of given[kind] || []) {
      const m = /^(Bash|Edit|Write)\((.*)\)$/.exec(rule);
      if (m) rules[kind].push([m[1].toLowerCase(), glob(m[2])]);
    }
  }
  return rules;
}

const READ_ONLY = new Set(["read", "grep", "find", "ls"]);

// "allow", "deny" or "ask" for one tool call in `cwd`.
export function decide(rules, toolName, input, cwd, sessionAllow = []) {
  if (READ_ONLY.has(toolName)) return "allow";
  if (toolName === "bash") {
    const command = String(input?.command ?? "").trim();
    if (rules.deny.some(([k, re]) => k === "bash" && re.test(command))) return "deny";
    if (rules.allow.some(([k, re]) => k === "bash" && re.test(command))) return "allow";
    if (sessionAllow.some((re) => re.test(command))) return "allow";
    return "ask";
  }
  if (toolName === "edit" || toolName === "write") {
    const file = path.resolve(cwd, String(input?.path ?? ""));
    const rel = "./" + path.relative(cwd, file);
    if (rules.deny.some(([k, re]) => k === toolName && re.test(rel))) return "deny";
    // Inside the session's own folder, as Orbit's worktrees are.
    if (!rel.startsWith("./..") && !path.isAbsolute(path.relative(cwd, file))) return "allow";
    if (rules.allow.some(([k, re]) => k === toolName && re.test(rel))) return "allow";
    return "ask";
  }
  return "ask";
}

// "cargo test --all" -> "cargo test *": what "always" allows for the session.
export function family(command) {
  const words = command.trim().split(/\s+/).slice(0, 2);
  return `${words.join(" ")} *`;
}

export default function (pi) {
  const rules = parseRules(process.env.ORBIT_PI_RULES);
  const sessionAllow = [];

  pi.on("session_start", (_event, ctx) => {
    if (enabled(ctx) && ctx.isIdle?.() === false) report("working");
  });

  pi.on("agent_start", (_event, ctx) => {
    if (enabled(ctx)) report("working");
  });

  pi.on("agent_settled", (_event, ctx) => {
    if (!enabled(ctx) || ctx.isIdle?.() !== true) return;
    report("turn");
  });

  pi.on("tool_call", async (event, ctx) => {
    if (!enabled(ctx)) return;
    const verdict = decide(rules, event.toolName, event.input, ctx.cwd, sessionAllow);
    if (verdict === "allow") return;
    if (verdict === "deny") {
      return { block: true, reason: "Orbit's rules for this session refuse it." };
    }
    const what =
      event.toolName === "bash" ? String(event.input?.command ?? "") : `${event.toolName} ${event.input?.path ?? ""}`;
    report("needs-approval", what);
    const options = ["Allow", "Decline"];
    const more = event.toolName === "bash" ? `Allow \`${family(what)}\` for this session` : undefined;
    if (more) options.splice(1, 0, more);
    const choice = await ctx.ui.select(`Allow ${event.toolName}?\n${what}`, options);
    report("working");
    if (choice === more) {
      sessionAllow.push(glob(family(what)));
      return;
    }
    if (choice !== "Allow") return { block: true, reason: "Declined in Orbit's pi session." };
  });
}
