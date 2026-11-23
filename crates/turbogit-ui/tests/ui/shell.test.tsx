import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n2/owner/shots";
const SLOW = { timeout: 20000 };

async function shellFromRecent(name: string) {
  const app = await open("../..", OPEN);
  await app.waitFor(`id:welcome:recent:${name}`, SLOW);
  await app.press(`id:welcome:recent:${name}`);
  await app.waitFor("id:tab:Log", SLOW);
  return app;
}

test("lane 1: the first screen at the trunk window", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("id:welcome:recents", SLOW);
  await app.screenshot(`${SHOTS}/n2-regression.png`);
});

test("lane 2: opening a recent project from Welcome enters the shell", async () => {
  const app = await shellFromRecent("alpha");
  await app.waitFor("text:1 total");
  expect(app.shows("text:Changes")).toBe(true);
  expect(app.shows("text:Log")).toBe(true);
  await app.screenshot(`${SHOTS}/n2-open-project.png`);
});

test("lane 3: the five tabs paint their routes and the activity filter moves", async () => {
  const app = await shellFromRecent("alpha");
  await app.press("id:tab:Commit");
  await app.waitFor("id:route:changes");
  await app.press("id:tab:Branches");
  await app.waitFor("id:route:branches");
  await app.press("id:tab:Worktrees");
  await app.waitFor("id:route:worktrees");
  await app.press("id:tab:Submodules");
  await app.waitFor("id:route:submodules");
  await app.press("id:tab:Log");
  await app.waitFor("text:Commits");

  await app.press("id:workspace:switch");
  await app.waitFor("id:workspace:picker");
  await app.press("id:workspace:row-current");
  await app.waitFor("id:workspace:picker", { state: "gone" });

  await app.press("id:activity:toggle");
  await app.waitFor("text:No activity yet");
  await app.press("id:activity:repo-filter");
  await app.waitFor("text:alpha");
  await app.screenshot(`${SHOTS}/n2-shell-tabs.png`);
});

test("lane 7: a multi-root workspace paints one gutter per root and filters one", async () => {
  const app = await shellFromRecent("ws");
  await app.waitFor("text:/2 repos indexed/", SLOW);
  await app.press("id:tab:Log");
  await app.waitFor("text:alpha commit 3");
  await app.waitFor("text:beta commit 3");

  await app.press("id:log:filter-paths");
  await app.waitFor("text:All repositories");
  await app.press("id:log:root-beta");
  await app.waitFor("text:Paths: 1 of 2 repos");
  expect(app.shows("text:beta commit 3")).toBe(false);
  await app.screenshot(`${SHOTS}/n2-multi-root.png`);
});

test("lane 9: a confirm and a toast answer, and the revert lands", async () => {
  const app = await shellFromRecent("alpha");
  await app.press("id:tab:Log");
  await app.waitFor("text:alpha change 5");

  await app.rightClick("id:log:list >> text:alpha change 5");
  await app.chooseMenuItem("Copy hash");
  await app.waitFor("text:/^Copied /");
  await app.screenshot(`${SHOTS}/n2-feedback.png`);

  app.onDialog(() => "OK");
  await app.rightClick("id:log:list >> text:alpha change 5");
  await app.chooseMenuItem("Revert commit");
  await app.waitFor("text:/^Revert/", SLOW);
});
