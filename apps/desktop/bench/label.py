"""Étiquettes de précision : rejoue le tirage d'échantillons et lui applique
les verdicts saisis à la main, puis écrit les étiquettes et les mesures.

usage :
  python3 -I bench/label.py <relevés.jsonl> <verdicts.json> <étiquettes.jsonl> [precision.json]

<verdicts.json> : {"regle": {"verdicts": "FFT…", "note": "…"}} ; le n-ième
caractère est le verdict du n-ième relevé tiré (T vrai positif, F faux
positif). Le tirage est celui de sample.py : déterministe par règle, un
relevé par dépôt à tour de rôle. Avec <precision.json>, les mesures de ces
règles y sont mises à jour (les autres règles sont gardées).
"""
import json
import random
import sys
from collections import defaultdict

SAMPLE = 20

def sample(findings, rule, n):
    repos = defaultdict(list)
    for r in findings:
        if r["rule"] == rule:
            repos[r["repo"]].append(r)
    rng = random.Random(f"herald-{rule}")
    order = sorted(repos)
    rng.shuffle(order)
    for repo in order:
        rng.shuffle(repos[repo])
    picked, depth = [], 0
    while len(picked) < n and any(len(repos[r]) > depth for r in order):
        for repo in order:
            if len(repos[repo]) > depth and len(picked) < n:
                picked.append(repos[repo][depth])
        depth += 1
    return picked, sum(len(v) for v in repos.values())

def main():
    findings_path, verdicts_path, labels_path = sys.argv[1:4]
    precision_path = sys.argv[4] if len(sys.argv) > 4 else None
    findings = [json.loads(line) for line in open(findings_path)]
    verdicts = json.load(open(verdicts_path))
    measures = {}
    with open(labels_path, "w") as out:
        for rule, entry in sorted(verdicts.items()):
            marks = entry["verdicts"]
            picked, total = sample(findings, rule, SAMPLE)
            if len(marks) > len(picked):
                sys.exit(f"{rule} : {len(marks)} verdicts pour {len(picked)} relevés")
            for finding, mark in zip(picked, marks):
                out.write(json.dumps({
                    "rule": rule,
                    "repo": finding["repo"],
                    "commit": finding["commit"],
                    "path": finding["path"],
                    "line": finding["line"],
                    "verdict": "vrai" if mark == "T" else "faux",
                }, ensure_ascii=False) + "\n")
            tp = marks.count("T")
            measures[rule] = {
                "true_positives": tp,
                "labeled": len(marks),
                "total": total,
                "note": entry.get("note", ""),
            }
            print(f"{rule:<28} {tp:>2}/{len(marks):<2} vrais  ({total} relevés)")
    if precision_path:
        try:
            current = json.load(open(precision_path))
        except FileNotFoundError:
            current = {"rules": {}}
        current["rules"].update(measures)
        current["rules"] = dict(sorted(current["rules"].items()))
        with open(precision_path, "w") as f:
            json.dump(current, f, ensure_ascii=False, indent=2)
            f.write("\n")

if __name__ == "__main__":
    main()
