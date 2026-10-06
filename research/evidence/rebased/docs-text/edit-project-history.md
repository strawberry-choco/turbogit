<!-- source: https://www.jetbrains.com/help/idea/edit-project-history.html, fetched 2026-10-04 -->
Edit Git project history | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Edit Git project history 

Git allows you to edit your project history. This is useful when you're working on a feature branch and want to clean it up and make it look the way you want before you share it with others. For example, you can edit commit messages, squash together smaller commits related to the same functionality, or split a commit that contains unrelated changes into separate commits, add changes to a previous commit, and so on. 
Avoid modifying the history for a remote branch with multiple contributors unless absolutely necessary, for example, if you accidentally pushed some sensitive data. 
Pushing modifications that rewrite a branch history to the remote repository will be rejected to prevent data loss, so you will have to use force push . 
You cannot modify the history for protected branches where git push --force is not allowed (configure protected branches in the Version Control | Git settings page Ctrl+Alt+S . Note that if a branch is marked as protected on GitHub, IntelliJ IDEA will automatically mark it as protected when you check it out. 
Also, you cannot perform actions that modify a branch history for commits that are not contained in the branch currently checked out. 

## Edit a commit message 

If the only thing you need to change is a commit message, you can edit it before you push this commit. 

Right-click the commit whose message you want to edit in the Log tab of the Git tool window Alt+9 and select Edit Commit Message from the context menu, or press F2 . 

In the dialog that opens, enter a new commit message and click OK . 
You can also modify the commit message right after committing by clicking Edit Commit Message in the notification about the successful commit. 

## Amend local commits 

Sometimes you may commit too early and forget to add some files, or notice an error in one of your unpushed commits that you want to fix without creating a separate commit. 
You can do this by using the Amend commit option that appends staged changes to the chosen local commit. As a result, you end up with a single commit instead of two different ones. 

In the Commit tool window Alt+0 , select the modified files containing the changes you want to add to one of your local commits. 

Select the Amend option and click to choose the local commit you want to modify. 

Click Amend Commit . 

## Amend any earlier commit 

For amending local commits, refer to Amend local commits . 
If you need to add changes to any earlier commit that has already been pushed, you can do this by using the fixup or the squash action. Both commands append staged changes to the selected commit, but handle commit messages differently: 

squash adds the new commit message to the original commit 

fixup discards the new commit message, leaving only the message from the original commit 
Both commands require a rebase because they change the commit hashes. 

In the Commit tool window Alt+0 , select the modified files containing the changes you want to append to an earlier commit. 

In the Log tab of the Git tool window Alt+9 , right-click the commit that you want to modify with your local changes and select Fixup or Squash Into from the context menu. 
You can also fixup or squash your changes when reviewing file history in the History tab of the Version Control tool window Alt+9 . 

Modify the commit message if you've chosen to squash changes. 

Click the arrow on the Commit button and choose Commit and Rebase . 

## Squash commits 

If you need to meld any two commits related to the same functionality, you can squash them into one for the sake of cleaner branch history. 

In the Log tab of the Git tool window Alt+9 select the commits that you want to combine into one and choose Squash Commits from the context menu. 

In the dialog that opens, edit the commit message (by default, it contains the messages from both commits) and click OK . 

Push Ctrl+Shift+K the changes to the remote branch. 

## Drop a commit 

You can discard a pushed commit in the current branch without creating an additional commit that reverts the changes . 

Select a commit you want to discard in the Log view and choose Drop Commit from the context menu. 

## Extract selected changes 

If you want to split the already committed changes into several commits, you can extract the changes in selected files into a separate commit. 

In the Log tab of the Git tool window Alt+9 , select the local branch and then the commit you want to split. 

In the Changed Files pane on the right, select the files with the changes you want to extract, right-click them, and select Extract Selected Changes to Separate Commit . 

In the dialog that opens, enter the new commit message if necessary. 
As a result, there will be two commits with new hashes. 
If the commit that you want to split was already pushed, it is still possible to split it. However, you will have to either use force push (cannot be performed in protected branches) or update your project first to sync the local branch with the remote tracked branch and then push the merge commit. 14 August 2026 Use tags to mark specific Git commits Git integration with issue trackers
