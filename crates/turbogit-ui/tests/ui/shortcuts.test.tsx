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

test("lane 4: the shortcuts window lists the keys the app runs", async () => {
  const app = await openProject("shortcuts");
  await app.press("id:toolbar:shortcuts");
  await app.waitFor("id:dialog:shortcuts", SLOW);
  await app.waitFor("id:shortcuts:table", SLOW);
  await app.waitFor("text:Ctrl+K", SLOW);
  expect(app.shows("text:Ctrl+Shift+K")).toBe(true);
  expect(app.shows("text:Ctrl+T")).toBe(true);
  expect(app.shows("text:Ctrl+Shift+A")).toBe(true);
  expect(app.shows("role:dialog[Shortcuts]")).toBe(true);
  expect(app.shows("role:button[Close]")).toBe(true);
  await app.screenshot(`${SHOTS}/n6-shortcuts.png`);
  await app.press("id:dialog:shortcuts:cancel");
  await app.waitFor("id:dialog:shortcuts", { state: "gone", timeout: 30000 });

  await app.press("id:tab:Log");
  await app.waitFor("text:Commits", SLOW);
  await app.keyboard.press("ctrl+k");
  await app.waitFor("id:route:changes", SLOW);
  expect(app.shows("id:tab:Commit")).toBe(true);

  await app.keyboard.press("ctrl+shift+a");
  await app.waitFor("id:dialog:command-palette", SLOW);
  expect(app.shows("text:Find Action")).toBe(true);

  await app.keyboard.press("ctrl+t");
  await app.waitFor("id:dialog:command-palette", SLOW);
});
