import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n2/owner/shots";
const SLOW = { timeout: 30000 };

async function logOver100k() {
  const app = await open("../..", OPEN);
  await app.waitFor("id:welcome:recent:repo-100k", SLOW);
  await app.press("id:welcome:recent:repo-100k");
  await app.waitFor("id:tab:Log", SLOW);
  await app.press("id:tab:Log");
  await app.waitFor("text:50 shown", SLOW);
  return app;
}

test("lane 4: scrolling the loaded 100k window keeps its rows and graph", async () => {
  const app = await logOver100k();
  await app.press("id:log:load-more");
  await app.waitFor("text:100 shown");
  await app.press("id:log:load-more");
  await app.waitFor("text:150 shown");

  await app.scroll("id:log:list", 4000);
  await app.wait(300);
  await app.waitFor("text:/^perf /");
  await app.scroll("id:log:list", -4000);
  await app.wait(300);
  await app.waitFor("text:perf 100000");
  await app.screenshot(`${SHOTS}/n2-log-scroll.png`);
});

test("lane 5: selecting a commit and scrolling to its parent lands on the row", async () => {
  const app = await logOver100k();
  await app.press("id:log:list >> text:perf 100000");
  await app.waitFor("text:/Parents/");
  await app.press("id:log:select-parent");
  await app.waitFor("text:/^393cb1d · Author/");
  await app.screenshot(`${SHOTS}/n2-scroll-to.png`);
});

test("lane 6: a remote branch scopes the listing and clearing restores it", async () => {
  const app = await logOver100k();
  await app.press("id:log:filter-branch");
  await app.press("id:log:band-Remote branches");
  await app.press("id:log:ref-origin/narrow");
  await app.waitFor("text:Branch: origin/narrow");
  await app.waitFor("text:perf 10000");
  expect(app.shows("text:perf 100000")).toBe(false);
  await app.screenshot(`${SHOTS}/n2-filter.png`);

  await app.press("id:log:remove-ref");
  await app.waitFor("id:log:ref-chip", { state: "gone" });
  await app.waitFor("text:perf 100000");
});

test("lane 8: the keyboard walks the listing from its top to its loaded end", async () => {
  const app = await logOver100k();
  await app.keyboard.press("end");
  await app.waitFor("text:perf 99951");
  await app.keyboard.press("home");
  await app.waitFor("text:perf 100000");
  await app.screenshot(`${SHOTS}/n2-keyboard.png`);
});
