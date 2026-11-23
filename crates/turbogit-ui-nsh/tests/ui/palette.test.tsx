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

async function palette(app: any) {
  await app.press("id:toolbar:palette");
  await app.waitFor("id:dialog:command-palette", SLOW);
}

test("lane 5: the palette lands on a branch, a commit, and a setting", async () => {
  const app = await openProject("palette");

  await palette(app);
  await app.type("label:Search actions", "Branches");
  await app.press("text:Branches…");
  await app.waitFor("id:route:branches", SLOW);
  await app.waitFor("id:branches:tree", SLOW);

  await palette(app);
  await app.type("label:Search actions", "Go to Log");
  await app.press("text:Go to Log");
  await app.waitFor("text:Commits", SLOW);

  await palette(app);
  await app.type("label:Search actions", "Settings");
  await app.press("text:Settings…");
  await app.waitFor("id:dialog:settings", SLOW);
  await app.waitFor("id:settings:body", SLOW);
  expect(app.shows("role:dialog[Settings]")).toBe(true);
  await app.screenshot(`${SHOTS}/n6-palette.png`);

  await app.press("id:dialog:settings:cancel");
  await app.waitFor("id:dialog:settings", { state: "gone", timeout: 30000 });

  await palette(app);
  await app.type("label:Search actions", "zzzz-no-action");
  await app.waitFor("text:No matching actions.", SLOW);
});
