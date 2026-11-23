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

async function customRoute(app: any) {
  await app.press("id:toolbar:n6-custom_command");
  await app.waitFor("id:route:custom_command", SLOW);
}

test("lane 6: a custom command runs on every repository", async () => {
  const app = await openProject("custom_command");
  await customRoute(app);
  await app.type("id:custom:input", "git branch lane/n6");
  await app.press("id:custom:open");
  await app.waitFor("id:bulk:preflight", SLOW);
  await app.waitFor("text:/Will run on all 2 repositories/", SLOW);
  await app.press("id:bulk:run");
  await app.waitFor("id:bulk:monitor", SLOW);
  await app.waitFor("text:/2 done/", SLOW);
  await app.screenshot(`${SHOTS}/n6-custom-command.png`);
  await app.press("id:bulk:close-monitor");
  await app.waitFor("id:bulk:monitor", { state: "gone", timeout: 30000 });
});

test("lane 9: a failing command reports its failure", async () => {
  const app = await openProject("custom_command");
  await customRoute(app);
  await app.type("id:custom:input", "git this-is-not-a-command");
  await app.press("id:custom:open");
  await app.waitFor("id:bulk:preflight", SLOW);
  await app.press("id:bulk:run");
  await app.waitFor("id:bulk:monitor", SLOW);
  await app.waitFor("text:/2 failed/", SLOW);
  expect(app.shows("id:bulk:monitor")).toBe(true);
  await app.screenshot(`${SHOTS}/n6-empty-error.png`);
});

test("lane 9: an empty repository explains itself", async () => {
  const app = await openProject("empty");
  await app.waitFor("id:status-bar", SLOW);
  await app.screenshot(`${SHOTS}/n6-empty-repo.png`);
});

test("lane 9: a detached HEAD still paints the shell", async () => {
  const app = await openProject("detached");
  await app.waitFor("id:status-bar", SLOW);
  expect(app.shows("text:/1 total/")).toBe(true);
  await app.screenshot(`${SHOTS}/n6-detached.png`);
});
