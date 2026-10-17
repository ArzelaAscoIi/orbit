#!/usr/bin/env python3
"""Tickets people triaged, as labelled cases for Laya's evaluation
(src/laya.rs eval_against_labelled_tickets). Reads LINEAR_API_KEY from .env
and writes target/laya-eval.json: ticket text, so it stays out of the repo.

    python3 scripts/laya-eval.py TEAM [TEAM ...]     # e.g. WEB API
    python3 scripts/laya-eval.py TEAM ... --profile profiles/<name>   # skip its sweeps
"""
import json, re, sys, urllib.request

key = next(l.split("=", 1)[1].strip().strip('"') for l in open(".env") if l.startswith("LINEAR_API_KEY="))
QUERY = """query($team: String!) { issues(first: 100, orderBy: createdAt,
  filter: {team: {key: {eq: $team}}, triagedAt: {null: false}}) {
  nodes { identifier title description priority team { key } labels { nodes { name } } } } }"""
out = []
args = sys.argv[1:]
profile = args[args.index("--profile") + 1] if "--profile" in args else None
teams = [a for i, a in enumerate(args) if not a.startswith("--") and (i == 0 or args[i - 1] != "--profile")]
if not teams:
    sys.exit("name the Linear team keys to read, e.g.: python3 scripts/laya-eval.py WEB API")
# Bot-filed tickets (a scanner's CVEs, the bot's sweeps) say nothing about how people write.
skip = [r"cve", r"security"]
if profile:
    p = json.load(open(f"{profile}/profile.json"))
    skip += [re.escape(s["prefix"]) for s in p.get("ktlo", {}).get("sweeps", [])]
    skip += [re.escape(c.lower()) for c in p.get("ktlo", {}).get("securityCreators", [])]
skip = re.compile("|".join(skip), re.I)
for team in teams:
    body = json.dumps({"query": QUERY, "variables": {"team": team}}).encode()
    request = urllib.request.Request("https://api.linear.app/graphql", data=body,
                                     headers={"Authorization": key, "Content-Type": "application/json"})
    for n in json.load(urllib.request.urlopen(request, timeout=60))["data"]["issues"]["nodes"]:
        labels = [l["name"].lower() for l in n["labels"]["nodes"]]
        if not n["description"] or len(n["description"]) < 80 or skip.search(n["title"]):
            continue
        kind = "bug" if "bug" in labels else "feature" if {"feature", "feature request", "improvement"} & set(labels) else None
        out.append({"id": n["identifier"], "note": (n["title"] + "\n\n" + n["description"])[:3000],
                    "team": n["team"]["key"], "priority": {1: "P1", 2: "P2", 3: "P3", 4: "P4"}.get(n["priority"]),
                    "kind": kind})
json.dump(out, open("target/laya-eval.json", "w"))
print(len(out), "tickets,", sum(1 for o in out if o["kind"]), "with a bug or feature label")
