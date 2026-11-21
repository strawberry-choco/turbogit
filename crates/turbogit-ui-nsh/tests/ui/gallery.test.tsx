import { expect, open, test } from "nsh/test";

// The gallery's live lanes at 1280x820 dark. Each test saves its PNG to the
// repo root for the review packet to copy into /tmp/swarm-n1/owner and
// media/nsh/N1-review. The regression comparison against the egui gallery
// and the hidpi lane are named gaps in the PR body, never passes here.

const OPEN = { width: 1280, height: 820, colorScheme: "dark" } as const;

test("lane 1: the gallery renders at 1280x820 dark", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("text:Queue event");
  await app.screenshot("n1-regression.png");
});

test("lane 2: tabbing walks the controls in reading order", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("text:Queue event");
  // Three stops of the keyboard: the first tab, a button, the tab list.
  await app.keyboard.press("tab");
  await app.keyboard.press("tab");
  await app.keyboard.press("tab");
  await app.screenshot("n1-focus-order.png");
});

test("lane 3: the keyboard presses controls", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("text:Queue event");
  await app.keyboard.press("tab");
  await app.keyboard.press("space");
  await app.keyboard.press("tab");
  await app.keyboard.press("return");
  await app.screenshot("n1-keyboard-press.png");
});

test("lane 4: the overlays open and Escape dismisses", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("text:Queue event");
  await app.press("id:demo:open-dialog");
  await app.waitFor("text:Rename branch");
  await app.screenshot("n1-overlays.png");
  await app.keyboard.press("escape");
  await app.waitFor("text:Open dialog");
  await app.press("text:Push toast");
  await app.waitFor("text:toast: pushed");
});

test("lane 5: typing covers single line, masked, and multiline", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("text:Queue event");
  await app.type("id:demo:input-single", "feature/log-lanes");
  await app.type("id:demo:input-masked", "hunter2");
  await app.type("id:demo:input-multiline", "first line\nsecond line");
  await app.screenshot("n1-inputs.png");
});

test("lane 7: resizing to 1024x700 keeps the sections intact", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("text:Queue event");
  await app.setViewport(1024, 700);
  await app.waitFor("text:Queue event");
  await app.screenshot("n1-resize.png");
});

test("lane 9: Chinese, an emoji, and a long label render", async () => {
  const app = await open("../..", OPEN);
  await app.waitFor("text:Queue event");
  const message = "中文提交信息 with an emoji 🚀";
  await app.type("id:demo:input-echo", message);
  await app.waitFor(`text:echo: ${message}`);
  await app.type(
    "id:demo:input-multiline",
    "中文提交信息 with an emoji 🚀",
  );
  await app.type(
    "id:demo:input-single",
    "a/very/long/unbroken/path/that/keeps/going/src/operations/rewrite/interactive/plan/rows/row.rs",
  );
  await app.screenshot("n1-cjk.png");
});

test("lane 10: reduced motion keeps every state change", async () => {
  const app = await open("../..", {
    ...OPEN,
    reducedMotion: true,
  });
  await app.waitFor("text:Queue event");
  await app.press("id:demo:checkbox-live");
  await app.waitFor("text:state: Checked");
  await app.press("text:Branches");
  await app.waitFor("text:tab: 2");
  await app.screenshot("n1-reduced-motion.png");
});
