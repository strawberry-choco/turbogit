import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n4/owner";
const SLOW = { timeout: 30000 };

async function openProject(name: string) {
  const app = await open("../..", OPEN);
  await app.waitFor("id:welcome:recents", SLOW);
  if (!app.shows(`id:welcome:recent:${name}`)) {
    await app.press("id:welcome:disclosure");
  }
  await app.waitFor(`id:welcome:recent:${name}`, SLOW);
  await app.press(`id:welcome:recent:${name}`);
  await app.waitFor("id:tab:Log", SLOW);
  return app;
}

async function openBranches(name: string) {
  const app = await openProject(name);
  await app.press("id:tab:Branches");
  await app.waitFor("id:route:branches", SLOW);
  await app.waitFor("id:branches:tree", SLOW);
  return app;
}

test("lane 5: add a worktree, then remove it behind the confirm", async () => {
  const app = await openBranches("branches-worktree");
  await app.press("id:tab:Worktrees");
  await app.waitFor("id:route:worktrees", SLOW);

  await app.press("id:worktree:add");
  await app.waitFor("id:dialog:new-worktree", SLOW);
  await app.type("id:dialog:new-worktree:path", "../branches-wt");
  await app.type("id:dialog:new-worktree:branch", "lane-wt");
  await app.press("id:dialog:new-worktree:action");
  await app.waitFor("id:worktree:remove-branches-wt", SLOW);
  await app.screenshot(`${SHOTS}/n4-worktree-added.png`);

  await app.onDialog(() => "OK");
  await app.press("id:worktree:remove-branches-wt");
  await app.waitFor("id:worktree:remove-branches-wt", {
    state: "gone",
    timeout: 30000,
  });
  await app.screenshot(`${SHOTS}/n4-worktree-removed.png`);
});

test("lane 6: edit both URL fields of a remote and add one", async () => {
  const app = await openBranches("branches-remotes");
  await app.press("id:picker:trigger");
  await app.waitFor("id:picker:list", SLOW);
  await app.press("id:picker:remotes");
  await app.waitFor("id:dialog:manage-remotes", SLOW);
  await app.waitFor("id:remote:row-origin", SLOW);

  await app.press("id:remote:edit-origin");
  await app.waitFor("id:remote:edit-fetch", SLOW);
  await app.clear("id:remote:edit-fetch");
  await app.type("id:remote:edit-fetch", "https://example.com/fetch.git");
  await app.clear("id:remote:edit-push");
  await app.type("id:remote:edit-push", "https://example.com/push.git");
  await app.press("id:remote:edit-save");
  await app.waitFor("text:/example.com\\/push.git/", SLOW);

  await app.type("id:remote:add-name", "team");
  await app.type("id:remote:add-fetch", "https://example.com/team.git");
  await app.press("id:remote:add");
  await app.waitFor("id:remote:row-team", SLOW);
  await app.screenshot(`${SHOTS}/n4-remotes.png`);
});

test("lane 7: push, then the up-to-date dialog offers nothing", async () => {
  const app = await openBranches("push-ok");
  await app.press("id:branches:push");
  await app.waitFor("id:sync:push-dialog", SLOW);
  await app.press("id:sync:row-check-push-ok");
  await app.press("text:Push");
  await app.wait(5000);

  await app.press("id:branches:push");
  await app.waitFor("id:sync:push-dialog", SLOW);
  await app.press("id:sync:row-check-push-ok");
  expect(app.shows("text:Nothing to update")).toBe(true);
  await app.screenshot(`${SHOTS}/n4-push-uptodate.png`);
});

test("lane 8: a rejected push offers the update step and the retry lands", async () => {
  const app = await openBranches("sync");
  await app.press("id:branches:push");
  await app.waitFor("id:sync:push-dialog", SLOW);
  await app.press("id:sync:row-check-sync");
  await app.press("text:Push");
  await app.waitFor("id:sync:rejection", SLOW);
  expect(app.shows("text:/Push to origin was rejected./")).toBe(true);
  expect(app.shows("text:Update and retry")).toBe(true);
  await app.screenshot(`${SHOTS}/n4-push-rejected.png`);

  await app.press("text:Update and retry");
  await app.waitFor("id:sync:rejection", { state: "gone", timeout: 30000 });
  await app.wait(3000);
  await app.screenshot(`${SHOTS}/n4-push-retried.png`);
});

test("lane 9: the merge preview names the commit, then the merge lands", async () => {
  const app = await openBranches("merge");
  await app.waitFor("id:branches:row:merge:feature", SLOW);
  await app.rightClick("id:branches:row:merge:feature");
  await app.chooseMenuItem("Merge into main");
  await app.waitFor("id:dialog:merge", SLOW);
  await app.waitFor("id:merge:preview-commits", SLOW);
  expect(app.shows("text:/Will create 1 merge commit/")).toBe(true);
  await app.screenshot(`${SHOTS}/n4-merge-preview.png`);

  await app.press("id:merge:strategy-Commit");
  await app.press("id:dialog:merge:action");
  await app.waitFor("id:dialog:merge", { state: "gone", timeout: 30000 });
  await app.wait(3000);
  await app.screenshot(`${SHOTS}/n4-merge-done.png`);
});
