#!/bin/sh
# Harvest machine-readable truth about the installed Rebased app bundle.
#
# Rebased is a JetBrains-based build (IntelliJ platform + the vcs-git plugin),
# so its bundle carries what a UI-parity project most wants and cannot guess:
# the action inventory, every user-facing string, the settings labels, and the
# default keymap. None of this needs the app to have ever been launched, and
# none of it needs Screen Recording or Accessibility permission.
#
# Usage:  research/evidence/rebased/extract.sh [path-to-Rebased.app]
# Output: research/evidence/rebased/<artifacts> + MANIFEST.txt
set -eu

APP="${1:-/Applications/Rebased.app}"
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$HERE"
PY="${PYTHON:-/Users/kye/.workbuddy/binaries/python/versions/3.13.12/bin/python3}"
[ -x "$PY" ] || PY="$(command -v python3)"

if [ ! -d "$APP/Contents" ]; then
  echo "no such app bundle: $APP" >&2
  exit 1
fi

version=$(defaults read "$APP/Contents/Info.plist" CFBundleShortVersionString)
build=$(defaults read "$APP/Contents/Info.plist" CFBundleGetInfoString)

{
  echo "app:      $APP"
  echo "version:  $version"
  echo "build:    $build"
  echo "date:     $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} > "$OUT/MANIFEST.txt"

"$PY" - "$APP" "$OUT" <<'PYEOF'
import os, re, sys, zipfile

app, out = sys.argv[1], sys.argv[2]

jars = []
for root, _, names in os.walk(os.path.join(app, "Contents")):
    for n in names:
        if n.endswith(".jar"):
            jars.append(os.path.join(root, n))


def open_jar(path):
    try:
        return zipfile.ZipFile(path)
    except Exception:
        return None


# --- 1. action inventory: every action id the bundled plugins declare -------
actions = {}
for p in jars:
    z = open_jar(p)
    if not z:
        continue
    for name in z.namelist():
        if not name.endswith("plugin.xml"):
            continue
        text = z.read(name).decode("utf-8", "ignore")
        for aid in re.findall(r'<action id="([^"]+)"', text):
            actions.setdefault(aid, os.path.basename(p))
with open(os.path.join(out, "actions.txt"), "w") as f:
    for aid in sorted(actions):
        f.write("%s\t%s\n" % (aid, actions[aid]))

# --- 2. user-facing copy: the localized string bundles ----------------------
WANT = {
    "GitBundle.properties",
    "VcsBundle.properties",
    "VcsLogBundle.properties",
    "GitFrontendBundle.properties",
}
strings_dir = os.path.join(out, "strings")
os.makedirs(strings_dir, exist_ok=True)
found = []
for p in jars:
    z = open_jar(p)
    if not z:
        continue
    for name in z.namelist():
        if not (name.startswith("messages/") and os.path.basename(name) in WANT):
            continue
        base = os.path.basename(name)
        data = z.read(name)
        with open(os.path.join(strings_dir, base), "wb") as fh:
            fh.write(data)
        entries = len([l for l in data.decode("utf-8", "ignore").splitlines()
                       if l.strip() and not l.startswith("#")])
        found.append((base, entries, os.path.basename(p)))
with open(os.path.join(out, "strings.txt"), "w") as f:
    for base, entries, jar in sorted(found):
        f.write("%s\t%d entries\tfrom %s\n" % (base, entries, jar))

# --- 3. key bindings: the platform default keymap ---------------------------
KEYMAP = "keymaps/$default.xml"
KEYMAP_MAC = "keymaps/Mac OS X 10.5+.xml"
keyfile = None
for p in jars:
    z = open_jar(p)
    if not z:
        continue
    names = z.namelist()
    entry = KEYMAP if KEYMAP in names else (KEYMAP_MAC if KEYMAP_MAC in names else None)
    if entry:
        with open(os.path.join(out, "keymap-default.xml"), "wb") as fh:
            fh.write(z.read(entry))
        keyfile = os.path.basename(p)
        break

def human(stroke):
    return (stroke.replace("meta ", "Cmd+").replace("ctrl ", "Cmd+")
            .replace("alt ", "Opt+").replace("shift ", "Shift+")
            .replace("pressed ", "").strip())

bound = []
if keyfile:
    text = open(os.path.join(out, "keymap-default.xml"), encoding="utf-8").read()
    for aid, body in re.findall(r'<action id="([^"]+)">(.*?)</action>', text, re.S):
        keys = [human(k) for k in
                re.findall(r'keyboard-shortcut first-keystroke="([^"]+)"', body)]
        if keys:
            bound.append((aid, ", ".join(keys)))
    with open(os.path.join(out, "keymap-bindings.txt"), "w") as f:
        for aid, keys in sorted(bound):
            f.write("%-40s %s\n" % (aid, keys))

# --- 4. settings labels: JetBrains searchable options -----------------------
options_dir = os.path.join(out, "settings-labels")
os.makedirs(options_dir, exist_ok=True)
optcount = 0
for p in jars:
    z = open_jar(p)
    if not z:
        continue
    for name in z.namelist():
        if "searchableOptions" in name and name.endswith(".json"):
            base = os.path.basename(name).lstrip("m-")
            with open(os.path.join(options_dir, base), "wb") as fh:
                fh.write(z.read(name))
            optcount += 1

# --- 5. report --------------------------------------------------------------
print("actions:            %d (%s)" % (len(actions), os.path.basename(out)))
print("string bundles:     %s" % ", ".join(
    "%s=%d" % (b, c) for b, c, _ in sorted(set(found))))
print("keymap source:      %s" % (keyfile or "not found"))
print("keymap bindings:    %d" % len(bound))
print("searchable option sets: %d" % optcount)
PYEOF

echo "manifest: $OUT/MANIFEST.txt"
