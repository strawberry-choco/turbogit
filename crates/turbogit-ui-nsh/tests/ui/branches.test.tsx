import { expect, open, test } from "nsh/test";

const OPEN = {
  width: 1280,
  height: 820,
  colorScheme: "dark",
  platform: "macos",
} as const;
const SHOTS = "/tmp/swarm-n4/owner";
const SLOW = { timeout: 30000 };

async function openBranches(name: string) {
  const app = await open("../..", OPEN);
  await app.waitFor("id:welcome:recents", SLOW);
  if (!app.shows(`id:welcome:recent:${name}`)) {
    await app.press("id:welcome:disclosure");
  }
  await app.waitFor(`id:welcome:recent:${name}`, SLOW);
  await app.press(`id:welcome:recent:${name}`);
  await app.waitFor("id:tab:Branches", SLOW);
  await app.press("id:tab:Branches");
  await app.waitFor("id:route:branches", SLOW);
  await app.waitFor("id:branches:tree", SLOW);
  return app;
}

test("lane 1: expand and collapse a remote, group by directory, and filter", async () => {
  const root = "branches-list";
  const app = await openBranches(root);
  await app.waitFor(`id:branches:row:${root}:hotfix`, SLOW);

  await app.press(`id:branches:band:${root}:remote`);
  await app.waitFor(`id:branches:band:${root}:remote-origin`, SLOW);
  await app.waitFor(`id:branches:row:${root}:remote-only`, SLOW);
  await app.press(`id:branches:band:${root}:remote-origin`);
  await app.waitFor(`id:branches:row:${root}:remote-only`, {
    state: "gone",
    timeout: 30000,
  });
  await app.press(`id:branches:band:${root}:remote-origin`);
  await app.waitFor(`id:branches:row:${root}:remote-only`, SLOW);
  await app.screenshot(`${SHOTS}/n4-branches-remote-expanded.png`);

  await app.press(`id:branches:band:${root}:remote`);
  await app.waitFor(`id:branches:row:${root}:remote-only`, {
    state: "gone",
    timeout: 30000,
  });
  await app.press(`id:branches:band:${root}:remote`);
  await app.waitFor(`id:branches:row:${root}:remote-only`, SLOW);

  await app.press("id:branches:settings");
  await app.waitFor("id:branches:settings-menu", SLOW);
  await app.press("id:branches:toggle-directory");
  await app.waitFor("text:/feature\\/deep\\/a/", SLOW);
  await app.screenshot(`${SHOTS}/n4-branches-directory-off.png`);
  await app.press("id:branches:toggle-directory");

  await app.type("id:branches:search", "hotfix");
  await app.waitFor(`id:branches:row:${root}:hotfix`, SLOW);
  expect(app.shows(`id:branches:row:${root}:feature/b`)).toBe(false);
  await app.screenshot(`${SHOTS}/n4-branches-filter.png`);
});

test("lane 2: check out a local branch and a remote-tracking branch", async () => {
  const root = "branches-checkout";
  const app = await openBranches(root);
  await app.waitFor(`id:branches:row:${root}:hotfix`, SLOW);

  await app.rightClick(`id:branches:row:${root}:hotfix`);
  await app.chooseMenuItem("Checkout");
  await app.waitFor(`id:branches:band:${root}:repo >> text:hotfix`, SLOW);

  await app.press(`id:branches:band:${root}:remote`);
  await app.waitFor(`id:branches:band:${root}:remote-origin`, SLOW);
  await app.waitFor(`id:branches:row:${root}:remote-only`, SLOW);
  await app.rightClick(`id:branches:row:${root}:remote-only`);
  await app.chooseMenuItem("Checkout");
  await app.waitFor(`id:branches:band:${root}:repo >> text:remote-only`, SLOW);
  await app.screenshot(`${SHOTS}/n4-branches-checkout-remote.png`);
});

test("lane 3: create, rename, delete a branch, then tag the tip", async () => {
  const root = "branches-mutate";
  const app = await openBranches(root);
  await app.waitFor(`id:branches:row:${root}:hotfix`, SLOW);

  await app.press("id:branches:new");
  await app.waitFor("id:dialog:new-branch", SLOW);
  await app.clear("id:dialog:new-branch:name");
  await app.type("id:dialog:new-branch:name", "lane/new");
  await app.press("id:dialog:new-branch:checkout");
  await app.press("id:dialog:new-branch:action");
  await app.waitFor(`id:branches:row:${root}:lane/new`, SLOW);

  await app.rightClick(`id:branches:row:${root}:lane/new`);
  await app.chooseMenuItem("Rename branch");
  await app.waitFor("id:dialog:rename-branch", SLOW);
  await app.clear("id:dialog:rename-branch:name");
  await app.type("id:dialog:rename-branch:name", "lane/renamed");
  await app.press("id:dialog:rename-branch:action");
  await app.waitFor(`id:branches:row:${root}:lane/renamed`, SLOW);

  await app.onDialog(() => "OK");
  await app.rightClick(`id:branches:row:${root}:lane/renamed`);
  await app.chooseMenuItem("Delete branch");
  await app.waitFor(`id:branches:row:${root}:lane/renamed`, {
    state: "gone",
    timeout: 30000,
  });

  await app.press("id:branches:tag");
  await app.waitFor("id:dialog:tag", SLOW);
  await app.type("id:dialog:tag:name", "v2.0");
  await app.press("id:dialog:tag:action");
  await app.waitFor("text:/Create tag v2.0/", SLOW);

  await app.press("id:tab:Log");
  await app.waitFor("text:Commits", SLOW);
  await app.press("id:tab:Branches");
  await app.waitFor("id:route:branches", SLOW);
  await app.waitFor("id:branches:tree", SLOW);
  await app.press("id:branches:band-tags");
  await app.waitFor("id:branches:tag-v2.0", SLOW);
  await app.screenshot(`${SHOTS}/n4-branches-tag.png`);
});

test("lane 4: compare a branch with the current branch", async () => {
  const root = "branches-compare";
  const app = await openBranches(root);
  await app.waitFor(`id:branches:row:${root}:feature/b`, SLOW);

  await app.rightClick(`id:branches:row:${root}:feature/b`);
  await app.chooseMenuItem("Compare with main");
  await app.waitFor("id:dialog:compare-branches", SLOW);
  await app.waitFor("id:dialog:compare:commits", SLOW);
  expect(app.shows("text:/1 commit\\(s\\) in 'feature\\/b'/")).toBe(true);
  await app.screenshot(`${SHOTS}/n4-branches-compare.png`);
});
