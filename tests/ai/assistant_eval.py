"""Evaluate the study assistant end to end on a real model, through the dev server's own
chat path (the API key stays in the server, stored as the app stores it). For each case
in cases.json: the answer, its reasoning, timings, tokens, cost, and two checks:

  quotes   each quotation in the answer (in "…" or “…”, three words or more) is word for
           word in the attached text, compared as the app's search compares (case,
           curly quotes, dashes, and "æ" set aside), or the answer says nearby that it
           quotes from memory
  refs     each reference the answer gives parses and names a verse the KJV has, or that
           an attached translation has in its own numbering

The checks are a first pass: read each miss (`misses`) before counting it as one. Most
of what's left after tuning is phrases used as terms in quotation marks, or nothing
(punctuation an answer normalised).

    cargo run --release -p kjv-devserver                 (serves http://127.0.0.1:1420)
    python tests/ai/assistant_eval.py run <provider-id> <model> <out-dir> [--thinking on|off] [--effort low|high|max] [--lookups] [--only name,name]
    python tests/ai/assistant_eval.py score <out-dir>
    python tests/ai/assistant_eval.py misses <out-dir> [name,name]

The provider id is the one the dev server's AI settings gave the provider. With
--lookups the model may look things up itself (read, search, lexicon): what it looked
up is kept with each answer, and its quotations are checked against that too. See docs/ASSISTANT.md for the cases, the results, and what was changed.
"""
import difflib
import json
import pathlib
import re
import sys
import time
import unicodedata
import urllib.request

SERVER = "http://127.0.0.1:1420"
CASES = pathlib.Path(__file__).with_name("cases.json")

# Per million tokens: (cache hit, cache miss, output), DeepSeek's off-peak prices in
# October 2026 (peak hours cost twice as much)
PRICES = {
    "deepseek-flash": (0.003, 0.15, 0.6),
    "deepseek-v4-pro": (0.022, 0.66, 1.98),
}


def post(name, body, timeout=600):
    req = urllib.request.Request(
        f"{SERVER}/api/{name}", data=json.dumps(body).encode(), method="POST", headers={"Content-Type": "application/json"}
    )
    return urllib.request.urlopen(req, timeout=timeout)


def call(name, body):
    with post(name, body) as r:
        return json.loads(r.read())


def fold(s):
    """As the app's search folds text (crates/library/src/text.rs)."""
    out = []
    for c in unicodedata.normalize("NFC", s):
        if c in "‘’‛ʼ":
            out.append("'")
        elif c in "“”":
            out.append('"')
        elif "‐" <= c <= "—":
            out.append("-")
        elif c in "æÆ":
            out.append("ae")
        elif c.isspace():
            out.append(" ")
        else:
            out.append(c.lower())
    return re.sub(r"\s+", " ", "".join(out))


# "John 3:16", "1 John 4:8", "Song of Solomon 2:1", "Romans 8:26-27" ("Matthew 15:21–16:23"
# is checked by where it starts)
REF = re.compile(r"\b((?:[1-3] ?)?[A-Z][a-z]+\.?(?: of [A-Z][a-z]+)? \d{1,3}:\d{1,3}(?:[-–]\d{1,3}(?![\d:]))?)")


def quotations(answer):
    """(start, end, text) of each quotation: “…” pairs, and straight quotes paired in
    order within a paragraph (so the text between two quotations isn't taken for one)."""
    found = []
    for m in re.finditer(r"“([^”\n]+)”", answer):
        found.append((m.start(), m.end(), m.group(1)))
    for para in re.finditer(r"[^\n]+", answer):
        line = para.group(0)
        marks = [i for i, c in enumerate(line) if c == '"']
        for a, b in zip(marks[0::2], marks[1::2]):
            found.append((para.start() + a, para.start() + b + 1, line[a + 1:b]))
    return [f for f in found if len(f[2].split()) >= 3]


def loose(s):
    """For comparing quotations: single and double quotes alike (a quotation inside a
    quotation changes them), and editors' bracketed footnotes set aside."""
    return re.sub(r"\s+", " ", re.sub(r"\s*\[[^\]]{0,200}\]", "", s.replace('"', "'"))).strip()


def check_quotes(answer, context):
    # Verse numbers start a passage's lines; a quotation across verses leaves them out
    ctx = loose(fold(re.sub(r"(?m)^\d{1,3} ", "", context)))
    results = []
    for start, end, raw in quotations(answer):
        q = re.sub(r"[*_]", "", raw).strip()
        # The marks of a quotation inside it, and the punctuation American style puts inside
        while True:
            t = q.strip("'‘’ ").rstrip(".,;:!?").lstrip(".,;:!? ")
            if t == q:
                break
            q = t
        # An ellipsis splits a quotation into pieces, each of which must be there
        pieces = [p.strip(" .,;:") for p in re.split(r"\.\.\.|…", q) if len(p.strip(" .,;:")) >= 8]
        ok = all(loose(fold(p)) in ctx for p in pieces) if pieces else True
        around = answer[max(0, start - 120): end + 60].lower()
        memory = "memory" in around or "not attached" in around
        results.append({"quote": q[:160], "found": ok, "from_memory_said": memory})
    return results


def printed_numbers(context):
    """Verse numbers a translation prints in its text, "(3-19)" (the JPS's own), as "3:19"."""
    return {f"{c}:{v}" for c, v in re.findall(r"\((\d{1,3})-(\d{1,3})\)", context)}


def check_refs(answer, bibles=("kjv",), context=""):
    seen = []
    printed = printed_numbers(context)
    for m in REF.finditer(answer):
        ref = m.group(1)
        if ref in [s["ref"] for s in seen]:
            continue
        cv = re.search(r"(\d{1,3}):(\d{1,3})(?:[-–](\d{1,3}))?$", ref)
        if cv and printed and all(f"{cv.group(1)}:{v}" in printed for v in range(int(cv.group(2)), int(cv.group(3) or cv.group(2)) + 1)):
            seen.append({"ref": ref, "ok": True})
            continue
        result = None
        for bible in dict.fromkeys(["kjv", *bibles]):
            try:
                call("context_parse", {"text": ref, "bible": bible})
                result = {"ref": ref, "ok": True}
                break
            except Exception as e:  # noqa: BLE001
                msg = e.read().decode() if hasattr(e, "read") else str(e)
                # A name the parser doesn't know as a book (a commentator's) isn't a reference
                result = {"ref": ref, "ok": "No book called" in msg, "error": msg[:120]}
                if "No book called" in msg:
                    break
        seen.append(result)
    return seen


def run_case(case, provider, model, thinking, effort, max_tokens, lookups=False):
    spec = case.get("context", {"passages": []})
    ctx = call("context_text", {"context": spec})
    messages = []
    turns = []
    for question in case["questions"]:
        messages.append({"role": "user", "content": question})
        args = {
            "providerId": provider,
            "kind": "openai",
            "baseUrl": "https://api.deepseek.com/v1",
            "model": model,
            "context": spec,
            "messages": messages,
            "maxTokens": max_tokens,
        }
        if thinking is not None:
            args["enableThinking"] = thinking
        if effort:
            args["effort"] = effort
        if lookups:
            args["lookups"] = True
            args["contextWindow"] = 1048576
        started = time.time()
        first_text = None
        text, reasoning, usage, reason, error = "", "", None, None, None
        found = []
        with post("ai_chat", {"id": f"eval-{time.time_ns()}", "args": args}, timeout=900) as r:
            for line in r:
                if not line.strip():
                    continue
                ev = json.loads(line)
                t = ev.get("type")
                if t == "text":
                    if first_text is None:
                        first_text = time.time() - started
                    text += ev["text"]
                elif t == "reasoning":
                    reasoning += ev["text"]
                elif t == "usage":
                    # Each round of looking up is a request of its own: their tokens add up
                    u = ev["usage"]
                    if usage and u.get("round"):
                        usage = {k: (usage.get(k) or 0) + (u.get(k) or 0) for k in ("inputTokens", "outputTokens", "cachedTokens")}
                    else:
                        usage = u
                elif t == "lookup":
                    found.append({"tool": ev["tool"], "label": ev["label"], "tokens": ev["tokens"], "failed": ev["failed"], "text": ev["text"]})
                    # What it wrote before looking up ends there, as the app shows it
                    if text.strip() and not text.endswith("\n\n"):
                        text = text.rstrip() + "\n\n"
                elif t == "done":
                    reason = ev.get("reason")
                elif t == "error":
                    error = ev.get("message")
        cost = None
        if usage and model in PRICES:
            hit, miss, out = PRICES[model]
            cached = usage.get("cachedTokens") or 0
            cost = (cached * hit + ((usage.get("inputTokens") or 0) - cached) * miss + (usage.get("outputTokens") or 0) * out) / 1e6
        turns.append({
            "question": question,
            "answer": text,
            "reasoning_words": len(reasoning.split()),
            "reasoning": reasoning,
            "usage": usage,
            "reason": reason,
            "error": error,
            "seconds": round(time.time() - started, 1),
            "first_text": round(first_text or 0, 2),
            "cost": cost,
            "answer_words": len(text.split()),
            "lookups": found,
        })
        messages.append({"role": "assistant", "content": text})
    return {"name": case["name"], "label": ctx["label"], "context_tokens": ctx["tokens"], "turns": turns}


def run(argv):
    provider, model, out_dir = argv[:3]
    opts = argv[3:]
    thinking, effort, only, max_tokens = None, None, None, 16000
    lookups = "--lookups" in opts
    for i, o in enumerate(opts):
        if o == "--thinking":
            thinking = opts[i + 1] == "on"
        elif o == "--effort":
            effort = opts[i + 1]
        elif o == "--only":
            only = set(opts[i + 1].split(","))
        elif o == "--max":
            max_tokens = int(opts[i + 1])
    out = pathlib.Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    for case in json.loads(CASES.read_text(encoding="utf-8")):
        if only and case["name"] not in only:
            continue
        result = run_case(case, provider, model, thinking, effort, max_tokens, lookups)
        (out / f"{case['name']}.json").write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
        for t in result["turns"]:
            u = t["usage"] or {}
            print(f"{case['name']:30} {t['seconds']:6.1f}s first words {t['first_text']:5.1f}s in {u.get('inputTokens')} (cached {u.get('cachedTokens')}) out {u.get('outputTokens')} ${t['cost'] or 0:.4f} {t['reason']}{' ERROR ' + t['error'] if t['error'] else ''}", flush=True)
            for f in t["lookups"]:
                print(f"    looked up ({f['tool']}): {f['label']} · {f['tokens']} tokens{' FAILED: ' + f['text'][:120] if f['failed'] else ''}", flush=True)
    score([out_dir])


def score(argv):
    run_dir = pathlib.Path(argv[0])
    cases = {c["name"]: c for c in json.loads(CASES.read_text(encoding="utf-8"))}
    totals = {"seconds": 0, "cost": 0, "quotes": 0, "quotes_ok": 0, "attached": 0, "attached_ok": 0, "refs": 0, "refs_ok": 0, "turns": 0, "first_text": 0, "think": 0, "answer": 0}
    for f in sorted(run_dir.glob("*.json")):
        r = json.loads(f.read_text(encoding="utf-8"))
        case = cases.get(r["name"])
        if not case:
            continue
        ctx = call("context_text", {"context": case["context"]})["text"]
        for t in r["turns"]:
            seen = ctx + "".join("\n" + f["text"] for f in t.get("lookups", []))
            quotes = check_quotes(t["answer"], seen)
            refs = check_refs(t["answer"], case["context"].get("translations", []), seen)
            bad_q = [q for q in quotes if not q["found"] and not q["from_memory_said"]]
            bad_r = [x for x in refs if not x["ok"]]
            totals["turns"] += 1
            for k, v in (("seconds", t["seconds"]), ("first_text", t["first_text"]), ("cost", t["cost"] or 0), ("think", t["reasoning_words"]), ("answer", t["answer_words"])):
                totals[k] += v
            totals["quotes"] += len(quotes)
            totals["quotes_ok"] += len(quotes) - len(bad_q)
            # Quotations of what is attached (not the cases that attach nothing, or not the passage asked about)
            if not case.get("unattached") or t.get("lookups"):
                totals["attached"] += len(quotes)
                totals["attached_ok"] += sum(q["found"] for q in quotes)
            totals["refs"] += len(refs)
            totals["refs_ok"] += len(refs) - len(bad_r)
            print(f"{r['name']:30} {t['seconds']:6.1f}s first words {t['first_text']:5.1f}s ${t['cost'] or 0:.4f} thinking {t['reasoning_words']}w answer {t['answer_words']}w quotes {len(quotes) - len(bad_q)}/{len(quotes)} refs {len(refs) - len(bad_r)}/{len(refs)}")
            for q in bad_q:
                print(f"      quote? {q['quote'][:110]}")
            for x in bad_r:
                print(f"      ref?   {x['ref']} {x.get('error', '')[:80]}")
    n = max(1, totals["turns"])
    print(
        f"\n{totals['turns']} answers: {totals['seconds'] / n:.1f}s each, first words after {totals['first_text'] / n:.1f}s, ${totals['cost']:.4f} in all; "
        f"thinking {totals['think'] / n:.0f} words, answer {totals['answer'] / n:.0f} words on average; "
        f"quotations of attached text word for word {totals['attached_ok']}/{totals['attached']}; "
        f"all quotations found or said to be from memory {totals['quotes_ok']}/{totals['quotes']}; references valid {totals['refs_ok']}/{totals['refs']}"
    )


def misses(argv):
    """Each quotation not found, beside the closest words of the attached text and the
    answer's words before it, to judge by hand."""
    run_dir = pathlib.Path(argv[0])
    only = argv[1].split(",") if len(argv) > 1 else None
    cases = {c["name"]: c for c in json.loads(CASES.read_text(encoding="utf-8"))}
    for f in sorted(run_dir.glob("*.json")):
        r = json.loads(f.read_text(encoding="utf-8"))
        if only and r["name"] not in only:
            continue
        ctx = call("context_text", {"context": cases[r["name"]]["context"]})["text"]
        words = re.findall(r"\S+", ctx)
        for t in r["turns"]:
            a = t["answer"]
            seen = ctx + "".join("\n" + f["text"] for f in t.get("lookups", []))
            words = re.findall(r"\S+", seen)
            for q in check_quotes(a, seen):
                if q["found"]:
                    continue
                qw = q["quote"].split()
                n = max(3, len(qw))
                key = [w.lower().strip(".,;:'\"“”‘’") for w in qw]
                best, at = 0, 0
                for i in range(0, max(1, len(words) - n)):
                    s = difflib.SequenceMatcher(None, key, [w.lower().strip(".,;:'\"“”‘’") for w in words[i:i + n]]).ratio()
                    if s > best:
                        best, at = s, i
                i = a.find(q["quote"][:30])
                print(f"== {r['name']}: {q['quote'][:140]}")
                print(f"   answer: …{a[max(0, i - 160):i].replace(chr(10), ' ')}⟦QUOTE⟧")
                print(f"   closest ({best:.2f}): {' '.join(words[at:at + n + 4])[:240]}")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    command, *rest = sys.argv[1:] or ["help"]
    {"run": run, "score": score, "misses": misses}.get(command, lambda _: print(__doc__))(rest)
