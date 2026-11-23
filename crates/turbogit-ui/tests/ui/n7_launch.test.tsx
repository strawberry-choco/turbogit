import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n7/owner";
const SLOW = { timeout: 30000 };

test("N7 live: the old recents file is read and the five tabs walk", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("id:welcome:recents", SLOW);

  // These two rows exist only in the recents file the egui build wrote to the
  // default config dir; seeing them proves the nsh build reads that file
  // unchanged.
  await app.press("id:welcome:disclosure");
  await app.waitFor("text:umbrella-1", SLOW);
  expect(app.shows("text:umbrella-4")).toBe(true);
  await app.screenshot(`${SHOTS}/n7-macos-welcome.png`);

  // `unshelve` is another row of that same file and a real repository, so
  // opening it exercises the upgrade path to a live shell.
  await app.press("id:welcome:recent:unshelve");
  await app.waitFor("id:tab:Log", SLOW);
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
  await app.screenshot(`${SHOTS}/n7-macos-launch.png`);
});
