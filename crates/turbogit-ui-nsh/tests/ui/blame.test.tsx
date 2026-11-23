import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n6/owner";
const SLOW = { timeout: 30000 };

async function openProject(name: string) {
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

test("lane 2: blame a 2k line file and jump to its commit", async () => {
  const app = await openProject("blame");
  await app.press("id:tab:Log");
  await app.waitFor("id:log:list", SLOW);
  await app.waitFor("text:change line 1000", SLOW);
  await app.press("text:change line 1000");
  await app.waitFor("id:file:alpha:big.txt", SLOW);
  await app.press("id:file:alpha:big.txt");
  await app.waitFor("id:log:file-blame", SLOW);
  await app.press("id:log:file-blame");
  await app.waitFor("id:dialog:blame", SLOW);
  await app.waitFor("id:blame:row:1", SLOW);
  await app.waitFor("text:LINE", SLOW);
  expect(app.shows("role:dialog[Blame]")).toBe(true);
  await app.screenshot(`${SHOTS}/n6-blame.png`);

  await app.press("id:blame:row:1");
  await app.waitFor("id:dialog:blame", { state: "gone", timeout: 30000 });
  await app.waitFor("text:Commits", SLOW);
});
