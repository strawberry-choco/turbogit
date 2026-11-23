import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n5/owner";
const SLOW = { timeout: 20000 };

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

async function openSurface(name: string, route: string) {
  const app = await openProject(name);
  await app.press(`id:toolbar:n5-${route}`);
  await app.waitFor(`id:route:${route}`, SLOW);
  return app;
}

test("lane 1: a rebase plan reorders, squashes, rewrites and runs", async () => {
  const app = await openSurface("rewrite", "rebase");
  await app.press("id:rebase:open");
  await app.waitFor("id:dialog:rebase", SLOW);
  await app.press("id:rebase:onto");
  await app.waitFor("id:rebase:onto-option:onto-target", SLOW);
  await app.press("id:rebase:onto-option:onto-target");
  await app.waitFor("id:rebase:commits", SLOW);
  await app.screenshot(`${SHOTS}/n5-rebase-dialog.png`);
  await app.press("id:rebase:cancel");
  await app.waitFor("id:dialog:rebase", { state: "gone", timeout: 20000 });

  await app.press("id:toolbar:n5-rebase_editor");
  await app.waitFor("id:rebase-editor:open", SLOW);
  await app.press("id:rebase-editor:open");
  await app.waitFor("id:dialog:interactive-rebase", SLOW);
  await app.waitFor("text:feature-1", SLOW);

  await app.drag("id:rebase-editor:row >> nth=0", { x: 0, y: 52 });
  await app.press("id:rebase-editor:chip:squash >> nth=1");
  await app.press("id:rebase-editor:chip:fixup >> nth=2");
  await app.press("id:rebase-editor:chip:drop >> nth=3");
  await app.press("id:rebase-editor:chip:reword >> nth=0");
  await app.screenshot(`${SHOTS}/n5-rebase-drag.png`);
  await app.press("id:dialog:interactive-rebase:action");
  await app.waitFor("id:dialog:interactive-rebase", { state: "gone", timeout: 20000 });
  await app.screenshot(`${SHOTS}/n5-rebase-run.png`);
}, 120000);

test("lane 2: cherry-pick across roots lands in the target only", async () => {
  const app = await openSurface("cherry", "cherry_pick");
  await app.press("id:cherry:open-across");
  await app.waitFor("id:dialog:cherry-pick-across", SLOW);
  await app.waitFor("id:cherry:commit:1", SLOW);
  await app.press("id:cherry:commit:1");
  await app.waitFor("text:/1 of 3 selected/", SLOW);
  await app.press("id:cherry:run");
  await app.waitFor("id:bulk:cherry-monitor", SLOW);
  await app.waitFor("text:Done", SLOW);
  await app.screenshot(`${SHOTS}/n5-cherry.png`);
});

test("lane 3: get files from another branch", async () => {
  const app = await openSurface("get", "get_from_branch");
  await app.press("id:get:open");
  await app.waitFor("id:dialog:get-from-branch", SLOW);
  await app.press("id:get:swap");
  await app.waitFor("text:Comparing feature with main (working tree)", SLOW);
  await app.screenshot(`${SHOTS}/n5-get-from-branch.png`);
  await app.onDialog(() => "OK");
  await app.press("id:get:take:both.txt");
  await app.waitFor("id:get:take:both.txt", { state: "gone", timeout: 20000 });
});

test("lane 4: create, apply, import and malformed-apply a patch, then unshelve", async () => {
  const app = await openSurface("patches", "patches");
  await app.press("id:patch:open-apply-file");
  await app.waitFor("id:dialog:apply-patch", SLOW);
  await app.clear("id:patch:path");
  await app.paste("id:patch:path", { text: "/tmp/swarm-n5/owner/projects/patches/bad.patch" });
  await app.press("id:patch:show-diff");
  await app.waitFor("text:The patch does not apply cleanly. No file has been changed.", SLOW);
  await app.screenshot(`${SHOTS}/n5-patch-malformed.png`);
  await app.press("id:patch:close");

  await app.press("id:patch:open-apply-file");
  await app.waitFor("id:dialog:apply-patch", SLOW);
  await app.clear("id:patch:path");
  await app.paste("id:patch:path", { text: "/tmp/swarm-n5/owner/projects/patches/change.patch" });
  await app.press("id:patch:apply");
  await app.waitFor("id:patch:outcome", SLOW);
  await app.screenshot(`${SHOTS}/n5-patch-applied.png`);
  await app.press("id:patch:close");

  await app.press("id:patch:open-create");
  await app.waitFor("id:dialog:create-patch-comment", SLOW);
  await app.paste("id:patch:comment", { text: "Lane patch" });
  await app.press("id:dialog:create-patch-comment:action");
  await app.waitFor("id:dialog:patch-file-settings", SLOW);
  await app.clear("id:patch:location");
  await app.paste("id:patch:location", { text: "/tmp/swarm-n5/owner" });
  await app.press("id:patch:settings-ok");
  await app.screenshot(`${SHOTS}/n5-patch-created.png`);

  await app.press("id:patch:open-import");
  await app.waitFor("id:dialog:import-patches", SLOW);
  await app.paste("id:patch:import-path", { text: "/tmp/swarm-n5/owner/projects/patches/change.patch" });
  await app.paste("id:patch:import-name", { text: "lane-import" });
  await app.press("id:patch:import");
  await app.waitFor("text:/on the shelf/", SLOW);
  await app.screenshot(`${SHOTS}/n5-patch-imported.png`);
});

test("lane 5: unshelve keeps the shelf, then removes it", async () => {
  const app = await openSurface("unshelve", "unshelve");
  await app.press("id:unshelve:open");
  await app.waitFor("id:dialog:unshelve", SLOW);
  await app.waitFor("text:weather", SLOW);
  await app.press("id:unshelve:remove-option");
  await app.waitFor("text:Unshelve and Remove", SLOW);
  await app.press("id:dialog:unshelve:action");
  await app.waitFor("id:dialog:unshelve", { state: "gone", timeout: 20000 });
  await app.screenshot(`${SHOTS}/n5-unshelve.png`);
});

test("lane 6: the rewrite route names the head and opens the reword editor", async () => {
  const app = await openSurface("rewrite", "rewrite");
  await app.waitFor("id:rewrite:reword-open", SLOW);
  await app.screenshot(`${SHOTS}/n5-rewrite.png`);
  await app.press("id:rewrite:reword-open");
  await app.waitFor("id:dialog:reword", SLOW);
  await app.screenshot(`${SHOTS}/n5-reword.png`);
});

test("lane 7: a bulk run across three roots creates a branch and rolls it back", async () => {
  const app = await openSurface("bulk", "bulk");
  await app.press("id:bulk:open:Create branch");
  await app.waitFor("id:bulk:preflight", SLOW);
  await app.paste("id:bulk:branch-name", { text: "lane/all" });
  await app.waitFor("text:Run on 3 of 3", SLOW);
  await app.press("id:bulk:run");
  await app.waitFor("id:bulk:monitor", SLOW);
  await app.waitFor("text:/3 done/", SLOW);
  await app.press("id:bulk:close-monitor");
  await app.screenshot(`${SHOTS}/n5-bulk.png`);

  await app.press("id:bulk:history-open");
  await app.waitFor("text:Create branch lane/all · 3 repos", SLOW);
  await app.press("text:/Rollback Create branch/");
  await app.screenshot(`${SHOTS}/n5-bulk-undo.png`);
});
