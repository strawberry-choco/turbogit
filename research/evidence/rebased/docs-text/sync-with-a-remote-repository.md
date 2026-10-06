<!-- source: https://www.jetbrains.com/help/idea/sync-with-a-remote-repository.html, fetched 2026-10-04 -->
Sync with a remote Git repository (fetch, pull, update) | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Sync with a remote Git repository (fetch, pull, update) 

Before you can share the results of your work by pushing your changes to the upstream , you need to synchronize with the remote repository to make sure your local copy of the project is up to date. You can do this in one of the following ways: fetch changes , pull changes , or update your project . 
The VCS widget indicates the number of incoming commits that have not yet been fetched (blue arrow icon next to the branch name) and the number of outgoing commits (green arrow icon next to the branch name). 

## Fetch changes 

When you fetch changes from the upstream, all new data from commits that were made since you last synced with the remote repository is downloaded into your local copy. This new data is not integrated into your local files, and changes are not applied to your code. 
Fetched changes are stored as a remote branch, which gives you a chance to review them before you merge them with your files. Since fetch does not affect your local development environment, this is a safe way to get an update of all changes to a remote repository. 
There are three ways to fetch changes from the upstream: 

Select Git | Fetch in the main menu. 

Open the VCS widget and click in the upper right corner. 

Enable automatic fetch of all remotes in your project: in Settings | Version Control | Git | Update , select the Fetch remote changes automatically option. 
This background task runs every 20 minutes by default (configurable via the git.update.incoming.info.time registry key). 
Watch this video to get a better view on how fetch operation is performed in IDE. 

## Update branch 

Use update if you need to sync a specific branch with its remote tracked branch. This is a convenient shortcut for fetching and subsequently applying changes to the selected branch. 

In the VCS widget or in the Branches pane of the Version Control tool window, select a branch and choose Update from the context menu. 
IntelliJ IDEA will pull changes from the remote branch and will rebase or merge them into the local branch depending on which update method is selected in Settings | Version Control | Git . 

## Pull changes 

If you need to get changes into the current branch from another branch instead of its remote tracked branch, use pull . When you pull , you not only download new data, but also integrate it into your local working copy of the project. 

In the main menu, go to Git | Pull . The Pull Changes dialog opens: 

If you have a multi-repository project, an additional drop-down appears letting you choose the repository. 

If you have several remotes defined for your project, select a remote from the list (by default, it is origin ). 

Select the branch from which you want to pull changes into the branch that is currently checked out. By default, the remote branch tracked by the current local branch is selected. If you specify a different branch, IntelliJ IDEA will remember your choice and display this branch by default in the future. 

If you need to pull with options, click Modify options and choose from the following: 

--rebase : after fetching the changes from a remote branch, IntelliJ IDEA will rebase local unpushed changes onto the fetched changes. 

--ff-only : the merge will be resolved only if it is possible to fast-forward. 

--no-ff : a merge commit will be created in all cases, even if the merge could be resolved as a fast-forward. 

--squash : a single commit with all pulled changes will be created on top of the current branch. 

--no-commit : a merge will be performed, but a merge commit will not be created so that you can inspect the result of the merge before committing. 

--no-verify : performs the merge while bypassing the pre-merge and commit-message hooks that normally run by default. 
For more information about pull options, refer to https://git-scm.com/docs/git-pull . 

Click Pull . 

## Update your project 

If you have several project roots, or want to fetch changes from all branches each time you sync with the remote repository, you may find updating your project a more convenient option. 
When you perform the update operation, IntelliJ IDEA fetches changes from all project roots and branches, and merges the tracked remote branches into your local working copy (equivalent to pull ). 
If your project contains submodules , and they are on a branch, they will also be updated automatically. 
If a submodule is in a detached HEAD state, IntelliJ IDEA will call git submodule update , which will check out the commit referenced in the root repository. This means an update will only be performed if the submodule reference changes in the root repo, or if a new submodule is added. 

In the main menu, go to Git | Update Project or press Ctrl+T . The Update Project dialog opens. 

Select the update type (this strategy will be applied to all roots that are under Git version control): 

Merge the incoming changes into the current branch : select this option to perform merge during the update. This is equivalent to running git fetch and then git merge , or git pull --no-rebase . 

Rebase the current branch on top of the incoming changes : select this option to perform rebase during the update. This is equivalent to running git fetch and then git rebase , or git pull --rebase (all local commits will be put on top of the updated upstream head). 

Reset to the Remote Branch : select this option to drop all local commits and align your branch exactly with the remote version. 
If you choose not to show the Update Project dialog in the future, and then want to modify the default update strategy later, go to the Version Control | Confirmation settings page Ctrl+Alt+S , select Update under Show options before , and modify the update strategy the next time you perform an update. 
When the update operation is completed, the Update Info tab is added to the Git tool window Alt+9 . It lists all commits that were made since the last sync with the remote, and lets you review the changes the same way as in the Log tab . By default, the tab opens automatically. To change this behavior, in Settings | Advanced Settings | Version Control | Git , clear the Automatically open the 'Update info' tab after updating the project checkbox. 
If you want to see a full list of all files modified since the last update, place the caret anywhere in the list of commits and press Ctrl+A . You can disable grouping to see a flat list: click on the toolbar in the Changed Files pane. 05 May 2026 Add files to Git and track changes Commit and push changes to Git repository
