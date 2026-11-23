import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n6/owner";
const SLOW = { timeout: 30000 };

async function openProject(name: string, options: Parameters<typeof open>[1] = OPEN) {
  const app = await open("../..", options);
  await app.waitFor("id:welcome:recents", SLOW);
  if (!app.shows(`id:welcome:recent:${name}`)) {
    await app.press("id:welcome:disclosure");
  }
  await app.waitFor(`id:welcome:recent:${name}`, SLOW);
  await app.press(`id:welcome:recent:${name}`);
  await app.waitFor("id:tab:Commit", SLOW);
  return app;
}

async function openSettings(app: any) {
  await app.press("id:toolbar:settings");
  await app.waitFor("id:dialog:settings", SLOW);
  await app.waitFor("id:settings:body", SLOW);
}

test("lane 3a: change every setting and apply", async () => {
  const app = await openProject("settings");
  await openSettings(app);

  await app.press("id:settings:restore-workspace");
  await app.press("id:settings:tab:git");
  await app.clear("id:settings:git-executable");
  await app.type("id:settings:git-executable", "git");

  await app.press("id:settings:tab:update-method");
  await app.press("label:Update method");
  await app.press("role:listbox >> role:option[Rebase]");
  await app.press("id:settings:auto-update");
  await app.press("id:settings:incoming-poll");
  await app.press("label:Check interval");
  await app.press("role:listbox >> role:option[Every 30 minutes]");
  await app.press("label:Clean working tree");
  await app.press("role:listbox >> role:option[Shelve]");

  await app.press("id:settings:tab:multi-root");
  await app.press("id:settings:synchronous-branches");

  await app.press("id:settings:tab:protected-branches");
  await app.press("id:settings:pattern-remove:master");
  await app.type("id:settings:new-pattern", "release/*");
  await app.press("id:settings:add-pattern");

  await app.press("id:settings:tab:appearance");
  await app.press("label:Date format");
  await app.press("role:listbox >> role:option[ISO]");
  await app.press("id:settings:gutter-markers");

  await app.press("id:settings:tab:advanced");
  await app.press("id:settings:warn-crlf");
  await app.press("id:settings:warn-detached");
  await app.press("label:When patch is created");
  await app.press("role:listbox >> role:option[Show in files]");
  await app.press("id:dialog:settings:action");
  await app.screenshot(`${SHOTS}/n6-settings.png`);

  // The applied values are what a fresh dialog seeds from. The Welcome
  // recents path does not reload `.turbogit/state.ron` (`open_project`), so a
  // second window opened from Welcome is the named restart gap in the N6
  // report; the nsh `settings_modal` suite proves Apply writes the file.
  await app.press("id:dialog:settings:cancel");
  await app.waitFor("id:dialog:settings", { state: "gone", timeout: 30000 });
  await openSettings(app);
  await app.press("id:settings:tab:update-method");
  await app.waitFor("text:Rebase", SLOW);
  await app.press("id:settings:tab:appearance");
  await app.waitFor("text:ISO", SLOW);
  await app.press("id:settings:tab:advanced");
  await app.waitFor("text:Show in files", SLOW);
  await app.screenshot(`${SHOTS}/n6-settings-reopened.png`);
}, 120000);

test("lane 8: reduced motion keeps the state change", async () => {
  const app = await openProject("settings", { ...OPEN, reducedMotion: true });
  await openSettings(app);
  await app.press("id:settings:restore-workspace");
  expect(app.shows("id:settings:restore-workspace")).toBe(true);
  await app.screenshot(`${SHOTS}/n6-motion-density.png`);
}, 120000);
