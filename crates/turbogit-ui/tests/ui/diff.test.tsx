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

async function rows(app: any, label: string) {
  for (let i = 0; i < 40; i++) {
    if (app.shows(`label:${label}`)) {
      // Let the native cell settle on its final bounds before pressing it.
      await app.wait(300);
      return;
    }
    await app.wait(100);
  }
  await app.waitFor(`label:${label}`, SLOW);
}

async function bigDiff() {
  const app = await openRecent("big");
  await app.press("id:tab:Commit");
  await app.waitFor("id:route:changes", SLOW);
  await app.press("id:changes:expand-all");
  await app.waitFor("id:changes:row:big:huge.txt", SLOW);
  await rows(app, "huge.txt");
  await app.press("label:huge.txt");
  await app.waitFor("text:/huge.txt @ /", SLOW);
  await app.waitFor("id:diff:list", SLOW);
  await app.waitFor("text:/hunk 1\\/2/", SLOW);
  return app;
}

test("lane 7: scrolling a 20k line diff keeps the rows and the staged line", async () => {
  const app = await bigDiff();

  await app.press("id:diff:next");
  await app.waitFor("text:/hunk 2\\/2/", SLOW);
  await rows(app, "line 19000 CHANGED (unstaged)");
  await app.press("label:line 19000 CHANGED (unstaged)");
  await app.waitFor("label:line 19000 CHANGED (staged)", SLOW);

  await app.scroll("id:diff:list", 2500);
  await app.wait(200);
  await app.scroll("id:diff:list", -2500);
  await app.waitFor("label:line 19000 CHANGED (staged)", SLOW);
  await app.screenshot(`${SHOTS}/n3-large-diff.png`);
});

test("lane 3: staging from a large diff names the hunk and the line", async () => {
  const app = await bigDiff();
  await app.press("id:diff:stage-hunk:0");
  await app.waitFor("text:/· staged/", SLOW);
  await app.waitFor("id:diff:stage-hunk:1", SLOW);
  await app.screenshot(`${SHOTS}/n3-diff-stage.png`);
});

test("lane 4: a historical diff opened from the log carries no staging", async () => {
  const app = await openRecent("big");
  await app.press("id:tab:Log");
  await app.waitFor("text:Commits", SLOW);
  await app.waitFor("text:/twenty thousand lines/", SLOW);
  await app.press("text:/twenty thousand lines/");
  await app.waitFor("id:log:file-diff", SLOW);
  await app.press("id:log:file-diff");
  await app.waitFor("id:diff:window", SLOW);
  await app.waitFor("text:line 00001", SLOW);
  expect(app.shows("id:diff:stage-hunk:0")).toBe(false);
  await app.screenshot(`${SHOTS}/n3-log-diff.png`);
  await app.press("id:dialog:diff:action");
  await app.waitFor("id:diff:window", { state: "gone", timeout: 15000 });
});
