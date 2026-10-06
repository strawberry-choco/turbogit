#!/usr/bin/env python3
"""Harvest Rebased parity evidence and emit per-surface spec skeletons.

One command does four things, all offline except the fetch step:

  1. verify + fetch every documentation URL named in `surfaces.tsv`, and convert
     each page to plain text under `docs-text/`
  2. load the bundle action inventory from `actions-labeled.tsv`
  3. write, for every surface, `parity/NN-<surface>.md`: a spec skeleton whose
     evidence sections are already filled with the real action table and which
     source pages were verified, leaving the three judgment sections to fill in
  4. write `HARVEST-REPORT.md` listing what resolved and what did not

Nothing here interprets the documentation. Turning page text into a parity
decision is the agent pass described in `DISTILL.md`.

A 404 is a **rename candidate**, not a terminal result: in this project two of
four "dead" slugs were alive under other names (and a fifth surfaced later), so
on a 404 the script searches the *topic* — plus the outbound help-site links of
every page fetched successfully in the same run, because a live sibling's links
are where a renamed page's content goes — probes each candidate slug, and fetches
the first one that resolves. The outcome, whatever it is, lands in the report.

Usage:  research/evidence/rebased/harvest.py [output dir]
"""

import os
import re
import sys
import time
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from html.parser import HTMLParser
from textwrap import shorten

HERE = os.path.dirname(os.path.abspath(__file__))
DOC_BASE = "https://www.jetbrains.com/help/idea/"
# The HTML endpoint, not the JS one: it returns parseable static markup.
SEARCH_URL = "https://html.duckduckgo.com/html/?q="
HELP_LINK = re.compile(
    r"href=\"https://www\.jetbrains\.com/help/idea/([a-z0-9\-]+)\.html\""
)


def today():
    return datetime.now(timezone.utc).strftime("%Y-%m-%d")


# ---------------------------------------------------------------- manifest ---

def load_manifest(path):
    """Read `surfaces.tsv`.

    `utf-8-sig` is not decoration: a BOM before the first `#` makes the leading
    comment line look like a data row, which then fails to split into columns.
    Malformed rows are skipped with a warning rather than aborting the run, so
    one bad line cannot cost a whole harvest.
    """
    surfaces = []
    with open(path, encoding="utf-8-sig") as f:
        for lineno, raw in enumerate(f, start=1):
            line = raw.rstrip("\n")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            cols = line.split("\t")
            if len(cols) != 4:
                print(f"  skip {path}:{lineno} — {len(cols)} columns, need 4",
                      file=sys.stderr)
                continue
            sid, title, slugs, prefixes = cols
            surfaces.append({
                "id": sid.strip(),
                "title": title.strip(),
                "slugs": [s.strip() for s in slugs.split(",") if s.strip()],
                "prefixes": [p.strip() for p in prefixes.split(",") if p.strip()],
            })
    return surfaces


# ------------------------------------------------------------- html -> text ---

SKIP = {"script", "style", "nav", "footer", "svg"}
HEADINGS = {"h1": "# ", "h2": "## ", "h3": "### ", "h4": "#### "}


class Extract(HTMLParser):
    """Strip an IntelliJ help page to readable Markdown-ish text.

    The pages are heavy on chrome; only headings, paragraphs, list items and
    table cells survive, which is exactly what a flow description is made of.
    """

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.out = []
        self.hidden = 0

    def handle_starttag(self, tag, attrs):
        if tag in SKIP:
            self.hidden += 1
        elif tag in HEADINGS:
            self.out.append("\n\n" + HEADINGS[tag])
        elif tag in ("p", "li", "tr", "br"):
            self.out.append("\n")
        elif tag in ("td", "th"):
            self.out.append(" | ")

    def handle_endtag(self, tag):
        if tag in SKIP and self.hidden:
            self.hidden -= 1
        elif tag in HEADINGS:
            self.out.append("\n")

    def handle_data(self, data):
        if self.hidden:
            return
        text = data.strip()
        if text:
            self.out.append(text + " ")

    def text(self):
        body = "".join(self.out)
        body = re.sub(r"[ \t]{2,}", " ", body)
        body = re.sub(r"\n{3,}", "\n\n", body)
        # The help pages repeat themselves once for desktop and once for mobile.
        body = re.sub(r"\n(## .*?)\n(?=.*\n\1\n)", "\n", body, flags=re.S)
        return body.strip() + "\n"


def fetch(slug):
    url = DOC_BASE + slug + ".html"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, ""
    except Exception as e:
        return 0, ""


# ------------------------------------------------------------- 404 → rename ---

def search_topic(slug):
    """Search the topic, not the slug — the method that found two of four "dead"
    slugs here were alive. Returns candidate slugs, or None when the search
    itself could not run (a distinct outcome from "no hits": an unrecorded
    failed search reads as evidence of absence, which it is not)."""
    query = SEARCH_URL + urllib.parse.quote(
        f"site:jetbrains.com/help/idea {slug.replace('-', ' ')}")
    req = urllib.request.Request(query, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return sorted(set(HELP_LINK.findall(r.read().decode("utf-8", "replace"))))
    except Exception:
        return None


def resolve_rename(slug, fetched_html):
    """On a 404, build rename candidates from a topic search plus the outbound
    help-site links of every page this run fetched successfully, and probe each
    until one resolves. Returns (new_slug, note); new_slug is None when every
    candidate is dead or there were none."""
    candidates = set()
    search_failed = False
    hits = search_topic(slug)
    if hits is None:
        search_failed = True
    elif hits:
        candidates.update(hits)
    for html in fetched_html.values():
        candidates.update(HELP_LINK.findall(html))
    candidates.discard(slug)

    if not candidates:
        note = ("topic search " +
                ("unavailable" if search_failed else "returned no candidates"))
        return None, note + "; no sibling links held"
    for cand in sorted(candidates):
        time.sleep(0.5)
        status, html = fetch(cand)
        if status == 200 and html:
            return cand, "topic search + sibling links"
        note = (f"{len(candidates)} candidate(s) probed, none live"
                + ("; topic search unavailable" if search_failed else ""))
    return None, note


# ---------------------------------------------------------------- inventory ---

def load_actions(path):
    rows = []
    if not os.path.exists(path):
        return rows
    with open(path, encoding="utf-8") as f:
        next(f, None)
        for line in f:
            parts = line.rstrip("\n").split("\t")
            parts += [""] * (4 - len(parts))
            rows.append({"id": parts[0], "label": parts[1],
                         "description": parts[2], "key": parts[3]})
    return rows


def macro_to_mac(stroke):
    """JetBrains help pages encode Mac modifiers as Ctrl0/Alt0/Shift0."""
    return glyphs(stroke.replace("Ctrl0", "Cmd+").replace("Alt0", "Opt+")
                  .replace("Shift0", "Shift+"))


def glyphs(stroke):
    """`control Opt+N` from the keymap file and `Ctrl0K` from the docs are the
    same thing; one spelling belongs in a spec table. A row can carry several
    bindings (`F2, Shift F6`), so each is converted on its own and the list is
    rejoined — collapsing the comma into a `+` would invent a chord."""
    def one(s):
        out = (s.replace("Ctrl0", "⌘").replace("Alt0", "⌥").replace("Shift0", "⇧")
                .replace("BACK_QUOTE", "`").replace("pressed ", "")
                # the keymap file already spells them this way: `control Opt+N`
                .replace("Opt+", "⌥").replace("Cmd+", "⌘"))
        # Case-insensitive, and tolerant of the docs' other spelling `Shift0K`.
        out = re.sub(r"\b(meta|ctrl|control|cmd)\b\s*\+?\s*", "⌘", out, flags=re.I)
        out = re.sub(r"\b(alt|opt)\b\s*\+?\s*", "⌥", out, flags=re.I)
        out = re.sub(r"\bshift\b\s*\+?\s*", "⇧", out, flags=re.I)
        out = re.sub(r"\s+", "+", out.strip())
        out = re.sub(r"\+{2,}", "+", out).strip("+")
        return out

    return ", ".join(one(p) for p in stroke.split(", ") if p.strip())


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else HERE
    os.makedirs(out, exist_ok=True)
    text_dir = os.path.join(out, "docs-text")
    spec_dir = os.path.join(out, "parity")
    os.makedirs(text_dir, exist_ok=True)
    # Clear specs first: a surface renamed in surfaces.tsv must not leave its old
    # file behind, or two specs claim to be the same surface with different numbers.
    if os.path.isdir(spec_dir):
        for stale in os.listdir(spec_dir):
            os.remove(os.path.join(spec_dir, stale))
    os.makedirs(spec_dir, exist_ok=True)

    surfaces = load_manifest(os.path.join(HERE, "surfaces.tsv"))
    actions = load_actions(os.path.join(HERE, "actions-labeled.tsv"))
    date = today()
    report, unresolved, renames = [], [], []
    fetched_html = {}

    for n, surface in enumerate(surfaces, start=1):
        pages = []
        for slug in surface["slugs"]:
            status, html = fetch(slug)
            if status == 404:
                new_slug, note = resolve_rename(slug, fetched_html)
                renames.append((surface["id"], slug, new_slug, note))
                if new_slug:
                    time.sleep(0.5)
                    status, html = fetch(new_slug)
                    slug = new_slug
            if status == 200 and html:
                fetched_html[slug] = html
                body = Extract()
                body.feed(html)
                with open(os.path.join(text_dir, slug + ".md"), "w") as f:
                    f.write(f"<!-- source: {DOC_BASE}{slug}.html, fetched {date} -->\n")
                    f.write(body.text())
                pages.append({"slug": slug, "status": status, "chars": len(body.text())})
            else:
                pages.append({"slug": slug, "status": status, "chars": 0})
                unresolved.append((surface["id"], slug, status))

        rows = [a for a in actions
                if any(a["id"].startswith(p) or ("." + p) in a["id"]
                       for p in surface["prefixes"])]
        rows.sort(key=lambda a: a["id"])

        ok_pages = [p for p in pages if p["status"] == 200]
        path = os.path.join(spec_dir, f"{n:02d}-{surface['id']}.md")
        with open(path, "w") as f:
            f.write(f"# {surface['title']}\n\n")
            f.write(f"> Surface `{surface['id']}`. Generated {date} by `harvest.py`\n")
            f.write("> from installed-bundle evidence plus verified documentation.\n")
            f.write("> The three judgment sections below are for the agent pass;\n")
            f.write("> see `research/evidence/rebased/DISTILL.md`.\n\n")

            f.write("## Sources\n\n")
            if ok_pages:
                for p in ok_pages:
                    f.write(f"- {DOC_BASE}{p['slug']}.html — verified 200, "
                            f"{p['chars']} chars → `docs-text/{p['slug']}.md`\n")
            else:
                f.write("- **no documentation resolved** — this surface needs runtime capture\n")
            for p in pages:
                if p["status"] != 200:
                    f.write(f"- `{p['slug']}` → HTTP {p['status']} (unresolved)\n")
            f.write("\n")

            f.write("## UI — what exists\n\n")
            if rows:
                f.write(f"{len(rows)} actions harvested from the installed bundle.\n\n")
                f.write("| Action id | Label | Key |\n| --- | --- | --- |\n")
                for a in rows:
                    f.write(f"| `{a['id']}` | {a['label'] or '—'} | "
                            f"{glyphs(a['key']) or '—'} |\n")
            else:
                f.write("_No bundle actions matched this surface's prefixes._\n")
            f.write("\nTODO: topology (panel/region layout), per-widget contents, and the\n"
                    "state matrix (default / hover / pressed / disabled / focus / selected /\n"
                    "progress) with a note on where it came from.\n\n")

            f.write("## UX — how it behaves\n\n")
            for q in ("How is it reached, and what is hidden behind a secondary "
                      "affordance?",
                      "Which verbs are destructive, and what do they confirm or "
                      "preflight?",
                      "What can be undone, from where, and for how long?",
                      "Which options are on by default, and what is remembered "
                      "between uses?",
                      "What feedback appears: progress, optimistic paint, "
                      "notification wording, and where does focus land after the "
                      "action?"):
                f.write(f"- TODO: {q}\n")
            f.write("\n")

            f.write("## Flow — how tasks chain\n\n")
            f.write("- TODO: entry points into this surface\n")
            f.write("- TODO: internal sequence, including dialog chaining\n")
            f.write("- TODO: what happens next on success\n")
            f.write("- TODO: what happens next on failure, and how the user recovers\n")
            f.write("- TODO: escape / dismiss semantics\n\n")

            f.write("## Divergences (intentional)\n\n")
            f.write("| Dimension | Rebased behaviour | TurboGit behaviour | Why |\n")
            f.write("| --- | --- | --- | --- |\n")
            f.write("| | | | |\n")

        report.append((surface["id"], n, len(rows), len(ok_pages), len(pages)))

    with open(os.path.join(out, "HARVEST-REPORT.md"), "w") as f:
        f.write(f"# Harvest report — {date}\n\n")
        f.write(f"- Rebased: {open(os.path.join(HERE, 'MANIFEST.txt')).read().strip().splitlines()[1].strip() if os.path.exists(os.path.join(HERE, 'MANIFEST.txt')) else 'run extract.sh'}\n")
        f.write(f"- surfaces: {len(surfaces)}\n")
        f.write(f"- bundle actions: {len(actions)}\n")
        f.write(f"- pages resolving: {sum(r[3] for r in report)} of "
                f"{sum(r[4] for r in report)}\n\n")
        f.write("| # | Surface | Actions | Docs OK / named |\n| --- | --- | --- | --- |\n")
        for sid, n, na, ok, total in report:
            f.write(f"| {n:02d} | {sid} | {na} | {ok} / {total} |\n")
        if unresolved:
            f.write("\n## Unresolved documentation\n\n")
            for sid, slug, status in unresolved:
                f.write(f"- `{surface_label(sid, slug, status)}` for `{sid}`\n")
        if renames:
            f.write("\n## 404 rename candidates\n\n")
            f.write("A 404 is a rename candidate, not a dead end: the topic was\n"
                    "searched and sibling pages' links were followed before giving\n"
                    "up (see SOURCES.md — two of four \"dead\" slugs here were\n"
                    "alive).\n\n")
            for sid, slug, new_slug, note in renames:
                if new_slug:
                    f.write(f"- `{slug}` → **`{new_slug}`** (resolved) — `{sid}`\n")
                else:
                    f.write(f"- `{slug}` unresolved: {note} — `{sid}`\n")

    print(f"surfaces:        {len(surfaces)}")
    print(f"bundle actions:  {len(actions)}")
    print(f"pages fetched:   {sum(r[3] for r in report)} / {sum(r[4] for r in report)}")
    print(f"specs written:   {os.path.join(out, 'parity')}")
    print(f"page text:       {text_dir}")
    print(f"report:          {os.path.join(out, 'HARVEST-REPORT.md')}")
    if unresolved:
        print(f"unresolved:      {len(unresolved)} see report")
    if renames:
        print(f"rename attempts: {len(renames)} see report")


def surface_label(sid, slug, status):
    return f"{DOC_BASE}{slug}.html → HTTP {status}"


if __name__ == "__main__":
    main()
