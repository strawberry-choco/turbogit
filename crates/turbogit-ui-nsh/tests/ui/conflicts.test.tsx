import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n5/owner";
const SLOW = { timeout: 60000 };

async function openConflicts() {
  const app = await open("../..", OPEN);
  await app.waitFor("id:welcome:recents", SLOW);
  if (!app.shows("id:welcome:recent:conflicts")) {
    await app.press("id:welcome:disclosure");
  }
  await app.waitFor("id:welcome:recent:conflicts", SLOW);
  await app.press("id:welcome:recent:conflicts");
  await app.waitFor("id:tab:Commit", SLOW);
  await app.press("id:toolbar:n5-conflicts");
  await app.waitFor("id:route:conflicts", SLOW);
  return app;
}

test("lane 1: a conflicting merge resolves in every pane", async () => {
  const app = await openConflicts();
  await app.waitFor("id:conflicts:resolve:conf.txt", SLOW);
  await app.press("id:conflicts:resolve:conf.txt");
  await app.waitFor("id:dialog:resolve-conflicts", SLOW);
  await app.waitFor("id:conflicts:take:0:1", SLOW);
  await app.press("id:conflicts:take:0:1");
  expect(app.texts()).toContain("yours");
  await app.press("id:conflicts:next");
  await app.press("id:conflicts:take:1:2");
  await app.press("id:conflicts:discard-accept");
  expect(app.texts()).toContain("0 conflicts remaining");
  await app.screenshot(`${SHOTS}/n5-conflict.png`);
  await app.press("id:conflicts:apply");
  await app.waitFor("id:conflicts:resolve:conf.txt", { state: "gone", timeout: 20000 });
  await app.screenshot(`${SHOTS}/n5-conflict-done.png`);
}, 90000);

test("lane 2: the merge editor resolves and applies its file", async () => {
  const app = await openConflicts();
  await app.waitFor("id:conflicts:merge:second.txt", SLOW);
  await app.press("id:conflicts:merge:second.txt");
  await app.waitFor("id:dialog:merge-editor", SLOW);
  await app.waitFor("id:conflicts:merge-take:1:1", SLOW);
  await app.press("id:conflicts:merge-take:1:1");
  expect(app.texts().join(" ")).toContain("0 conflicts remaining");
  await app.screenshot(`${SHOTS}/n5-merge-editor.png`);
  await app.press("id:conflicts:merge-apply");
  await app.waitFor("id:dialog:merge-editor", { state: "gone", timeout: 20000 });
  await app.waitFor("id:conflicts:merge:second.txt", { state: "gone", timeout: 20000 });
}, 90000);
