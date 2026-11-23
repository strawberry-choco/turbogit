import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n3/owner";
const SLOW = { timeout: 30000 };

async function openRecent(name: string) {
  const app = await open("../..", OPEN);
  await app.waitFor("id:welcome:recents", SLOW);
  if (!app.shows(`id:welcome:recent:${name}`)) {
    await app.press("id:welcome:disclosure");
  }
  await app.waitFor(`id:welcome:recent:${name}`, SLOW);
  await app.press(`id:welcome:recent:${name}`);
  await app.waitFor("id:tab:Commit", SLOW);
  return app;
}

/** Waits until the platform has a native element for `label`: the live host
 * builds its native tree a few frames after the model paints a virtual-list
 * cell, and a lookup before it is a hard miss rather than a retry. */
async function rows(app: any, label: string) {
  for (let i = 0; i < 40; i++) {
    if (app.shows(`label:${label}`)) return;
    await app.wait(100);
  }
  await app.waitFor(`label:${label}`, SLOW);
}

/** Selects a changed-file row and waits for its preview: a native row press
 * can land before the cell's bounds settle, so it is retried until the
 * preview names the file. */
async function selectRow(app: any, label: string) {
  for (let attempt = 0; attempt < 12; attempt++) {
    await rows(app, label);
    await app.press(`label:${label}`);
    for (let i = 0; i < 15; i++) {
      if (app.shows(`text:/${label} @ /`)) return;
      await app.wait(100);
    }
  }
  await app.waitFor(`text:/${label} @ /`, SLOW);
}

async function commitTab(name: string, firstFile = "alpha.txt") {
  const app = await openRecent(name);
  await app.press("id:tab:Commit");
  await app.waitFor("id:route:changes", SLOW);
  await app.press("id:changes:expand-all");
  await app.waitFor("id:changes:list", SLOW);
  await rows(app, firstFile);
  return app;
}

test("lane 2: the Commit tab paints the bucket, group, filter, and selection", async () => {
  const app = await commitTab("changes");
  expect(app.shows("text:Changes")).toBe(true);
  await selectRow(app, "lines.txt");
  await app.waitFor("id:diff:toolbar", SLOW);
  await app.screenshot(`${SHOTS}/n3-changes-list.png`);
});

test("lane 3: stage one line, then a hunk, then a whole file", async () => {
  const app = await commitTab("stage", "lines.txt");
  await selectRow(app, "lines.txt");
  await app.waitFor("id:diff:list", SLOW);
  await app.waitFor("text:/hunk 1\\/2/", SLOW);

  await app.press("id:diff:stage-hunk:1");
  await app.waitFor("text:/· staged/", SLOW);

  await app.press("id:diff:del:2");
  await app.waitFor("text:/· part/", SLOW);

  await rows(app, "Stage all of lines.txt (partially staged)");
  await app.press("label:Stage all of lines.txt (partially staged)");
  await app.waitFor("label:Unstage lines.txt (staged)", SLOW);
  await app.screenshot(`${SHOTS}/n3-stage.png`);
});

test("lane 4: discarding behind the confirm restores the file", async () => {
  const app = await commitTab("discard", "a.txt");
  await selectRow(app, "b.txt");
  await app.press("id:changes:rollback");
  await app.waitFor("text:/Discard changes to 1 file/", SLOW);
  expect(app.shows("text:Discard 1-file changes")).toBe(true);
  await app.screenshot(`${SHOTS}/n3-discard.png`);

  await app.press("text:Discard 1-file changes");
  await app.waitFor("label:b.txt", { state: "gone", timeout: 30000 });
});

test("lane 5: a CJK message with an emoji commits and paints in the log", async () => {
  const app = await commitTab("commit", "notes.txt");
  await app.waitFor("id:commit:message", SLOW);
  const subject = "修复提交视图 🚀";
  await app.type("id:commit:message", subject);
  await app.wait(600);
  await app.press("id:commit:action");
  await app.waitFor("text:Recent:", SLOW);
  await app.screenshot(`${SHOTS}/n3-commit-cjk.png`);

  await app.press("id:tab:Log");
  await app.waitFor("text:Commits", SLOW);
  await app.waitFor(`text:${subject}`, SLOW);
});

test("lane 6: amending rewrites the last commit instead of adding one", async () => {
  const app = await commitTab("amend", "notes.txt");
  await app.waitFor("id:commit:message", SLOW);
  await app.type("id:commit:message", "amended subject");
  await app.press("id:commit:amend");
  await app.wait(600);
  await app.press("id:commit:action");
  await app.waitFor("text:Recent:", SLOW);
  await app.screenshot(`${SHOTS}/n3-amend.png`);

  await app.press("id:tab:Log");
  await app.waitFor("text:Commits", SLOW);
  await app.waitFor("text:amended subject", SLOW);
});

test("lane 8: the image and binary changes paint their designed panes", async () => {
  const app = await commitTab("panes", "image.png");
  await selectRow(app, "image.png");
  await app.waitFor("text:/12×12/", SLOW);
  await app.waitFor("text:/6×6/", SLOW);
  await app.screenshot(`${SHOTS}/n3-image.png`);

  await selectRow(app, "blob.bin");
  await app.waitFor("text:/Binary file changed/", SLOW);
  await app.screenshot(`${SHOTS}/n3-binary.png`);
});

test("lane 9: the shelf and stash panes paint their empty states", async () => {
  const app = await commitTab("panes", "image.png");
  await app.press("text:Shelf");
  await app.waitFor("text:SHELVES", SLOW);
  expect(app.shows("text:/Nothing is shelved/")).toBe(true);
  await app.screenshot(`${SHOTS}/n3-shelf.png`);

  await app.press("text:Stash");
  await app.waitFor("text:STASHES", SLOW);
  expect(app.shows("text:/Nothing is stashed/")).toBe(true);
});
