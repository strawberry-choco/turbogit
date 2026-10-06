<!-- source: https://www.jetbrains.com/help/idea/undo-changes.html, fetched 2026-10-05 -->
Undo changes in Git repository | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Undo changes in Git repository 

### Revert uncommitted changes 

You can always undo the changes you've made locally before you commit them: 

In the Commit tool window ( Alt+0 ) , select one or more files that you want to revert, and select Rollback from the context menu, or press Ctrl+Alt+Z . 

In the dialog that opens, check the list of files to be reverted. 
Select the Delete local copies of added files checkbox to revert added files as well as your changes in modified ones. 
All changes made to the selected files since the last commit will be discarded, and they will disappear from the active changelist. 

### Unstage files 

By default, IntelliJ IDEA uses the changelists concept where modified files are staged automatically. 

If a file is already under version control, and you do not want to commit it, you can: 

Remove it from the commit : do not select it in the Changes area of the Commit tool window. 

Move it to another changelist . 

If you are more used to the staging concept, select the Enable staging area option in the Version Control | Git settings page Ctrl+Alt+S . 
Also, by default IntelliJ IDEA suggests adding each newly created file under version control. You can change this behavior in Settings | Version Control | Confirmation using When files are created and When files are deleted settings respectively. 

### Undo the last commit 

IntelliJ IDEA allows you to undo the last commit in the current branch. 
You cannot undo a commit if it was pushed to a protected branch, that is a branch to which force --push is not allowed (configure protected branches in the Version Control | Git settings page Ctrl+Alt+S ) Note that if a branch is marked as protected on GitHub, IntelliJ IDEA will automatically mark it as protected when you check it out. 

Open the Git tool window Alt+9 and switch to the Log tab. 

Select the last commit in the current branch and choose Undo Commit from the context menu. 

In the Select Target Changelist dialog, select a changelist where the changes you are going to discard will be moved. You can either select an existing changelist from the Name list, or specify the name of a new changelist (the commit message is used by default). 
Type an optional comment. When the new changelist is submitted to the repository, this comment will appear in the Comment text area of the Commit Changes dialog. 

Select the Set active option if you want to make the changelist with the changes you are about to discard the active changelist. 

Select the Track context option if you want IntelliJ IDEA to remember your context and reload currently opened files in the editor when this changelist becomes active. 

### Revert a pushed commit 

If you notice an error in a specific commit that has already been pushed, you can revert that commit. This operation results in a new commit that reverses the effect of the commit you want to undo. Thus, the project history is preserved, as the original commit remains intact. 

Locate the commit you want to revert in the Log tab of the Git tool window Alt+9 , right-click it and select Revert Commit from the context menu. This option is also available from the context menu of a commit in the file History view. The Commit Changes dialog will open with an automatically generated commit message. 
If you apply this action to multiple commits selected in the Log view, a separate commit will be created to revert each of them. 

If the selected commit contains several files, and you only need to revert some of them, deselect the files you do not want to touch. 

Click Commit to commit a changeset that reverts the changes to the selected files in this particular commit. 

### Revert selected changes 

IntelliJ IDEA lets you undo selected changes from a pushed commit if this commit contains multiple files and you only need to revert some of them. 

In the Log view select the commit containing the changes you want to discard. 

In the Changed Files pane, right-click the file that you want to revert and select Revert Selected Changes from the context menu. 
This results in a new commit that reverses the changes you want to undo. 

### Drop a commit 

Unlike reverting a commit , which is reflected in the branch history, you can discard a pushed commit in the current branch without leaving any traces of the operation. 
Like any operation that rewrites a branch history, dropping a commit requires using force push and cannot be performed in protected branches (these can be configured in the Version Control | Git settings page Ctrl+Alt+S ). 

Select a commit you want to discard in the Log view and choose Drop Commit from the context menu. 

### Drop selected changes from a commit 

If you have committed changes in several files and now want to roll back some of these changes before pushing your commit, instead of dropping an entire commit , you can drop only some of these changes. Note that the dropped changes will be deleted. 

In the Log tab of the Git tool window Alt+9 , select the local branch and then the commit with the changes you want to drop. 

In the Changed Files pane on the right, select the files with the changes you want to drop, right-click them, and select Drop Selected Changes . 
If the commit was already pushed, it is still possible to drop the selected changes. However, you will have to either use force push (cannot be performed in protected branches) or update your project first to sync the local branch with the remote tracked branch and then push the merge commit. 

### Reset a branch to a specific commit 

If you notice an error in a set of recent commits and want to redo that part, you can roll back your repository to a specific state. This is done by resetting the current branch HEAD to a specified commit (and optionally resetting the index and working tree if you prefer not to reflect the undo in the history). 

Open the Version Control tool window Alt+9 and switch to the Log tab. 

Select the commit that you want to move HEAD onto and select Reset Current Branch to Here from the context menu. 

In the Git Reset dialog that opens, select how you want your working tree and the index to be updated and click Reset : 

Soft : all changes from commits that were made after the selected commit will be staged (that means they will be moved to the Commit window so that you can review them and commit later if necessary). 

Mixed : changes made after the selected commit will be preserved but will not be staged for commit. 

Hard : all changes made after the selected commit will be discarded (both staged and committed). 

Keep : committed changes made after the selected commit will be discarded, but local changes will be kept intact. 

### Get a previous revision of a file 

If you need to revert a single file instead of discarding a whole commit that includes changes to several files, you can return to a particular version of that file: 

Select the necessary file in any view (the Project tool window ( Alt+1 ) , the editor, the Commit window, and so on). 

Select Git | Selected File | Show History from the main menu or Git | Show History from the context menu of the selection. The History tab is added to the Git tool window showing the history for the selected file and allowing you to review and compare its revisions. 

When you've identified the revision you want to roll back to, select it in the list and choose Get from the context menu. 05 May 2026 Use patches Use tags to mark specific Git commits
