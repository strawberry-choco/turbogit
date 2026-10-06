<!-- source: https://www.jetbrains.com/help/idea/using-patches.html, fetched 2026-10-04 -->
Use patches | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Use patches 

Instead of committing your local changes, you can put them in a .patch file that you can apply to your sources later, send by email, and so on. Using patches is a convenient mechanism for sharing changes without checking them in a Git repository. 

### Create a patch from uncommitted changes 

In the Commit tool window ( Alt+0 ) , select the file or changelist that you want to include in the patch, and choose Create Patch from Local Changes from the context menu. 

In the dialog that opens, make sure that all the changes you want to include in the patch are selected, enter a commit comment (it will be used as the patch file name), and click Create Patch . 

In the Patch File Settings dialog, specify the following details: 

The patch file location: choose the default location or select the To clipboard option if you do not want to save the patch to a file. 

Base path: specify the path relative to which paths inside the patch file will be written. Normally, this is your project directory, but you may want to use a relative path, for example, if the modified files are stored inside your Git repository. 

Reverse patch: select this option if you want to create a patch that reverts the changes you have made. 

Encoding: select the encoding for the patch file from the drop-down list. 
If you do not need to save the patch to a file (for example, you want to send it by email instead), right-click the necessary file(s) in the Commit tool window ( Alt+0 ) and choose Copy as Patch to Clipboard from the context menu. 

### Create a patch from an entire commit 

In the Log tab of the Git tool window Alt+9 , locate the commit with the changes you want to include in the patch and select Create Patch from the context menu. 

In the Patch File Settings dialog, specify the following details: 

The patch file location: choose the default location or select the To clipboard option if you do not want to save the patch to a file. 

Base path: specify the path relative to which paths inside the patch file will be written. Normally, this is your project directory, but you may want to use a relative path, for example, if the modified files are stored inside your Git repository. 

Reverse patch: select this option if you want to create a patch that reverts the changes you have made. 

Encoding: select the encoding for the patch file from the drop-down list. 

### Create a patch from a file 

Select the necessary file in any view (the Project tool window ( Alt+1 ) , the editor, the Commit window, and so on). 

Select Git | Selected File | Show History from the main menu or Git | Show History from the context menu of the selection. The History tab is added to the Git tool window showing the history for the selected file and allowing you to review and compare its revisions. 

Right-click a revision and choose Create Patch from the context menu. 

### Apply patches 

In the main menu, select: 

Git | Patch | Apply patch to choose a saved .patch file. 

Git | Patch | Apply patch from Clipboard if you have copied the content of a patch file. 
For example, this is convenient when you receive a patch by email and do not want to save it. For Git format patches, IntelliJ IDEA extracts the commit message and the author and automatically fills the corresponding fields in the Commit tool window ( Alt+0 ) . 

In the Apply Patch dialog that opens, specify the path to the .patch file you want to apply. 
You can drag a file or an email attachment to any place in the editor. 

If the source code was edited after a patch was created, conflicts may arise. To check if your patch can be applied without conflicts, click Show Diff Ctrl+D . If there are conflicts, the corresponding lines will be highlighted in red. 

If necessary, click Change Directory Paths and choose: 

Map Base Directory : specify a directory relative to which file names in the patch file will be interpreted. You can map the base directory to a single file, directory, or selection. 

Remove Leading Directory : apply the changes to files located in different directories from the ones specified in the patch. Clicking this button removes one slash in the path to the target file. Click the button as many times as many leading directories you need to remove. The number of removed slashes is indicated in square brackets. 

Remove All Leading Directories : remove all leading directories with one click and apply the patch to the file with the specified name in the base directory. 

Restore Leading Directory : revert the last Remove Leading Directory action. Click the button as many times as many previously stripped leading directories you need to restore. 

Restore All Leading Directories : revert the Remove All Leading Directories action. 

Select the changelist to which you want to apply the patch or specify the name of a new changelist in the Name field, and enter a comment to this changelist (optionally). 

If you want to make this changelist active, select the Set active option. 

If you want IntelliJ IDEA to save the context of a task associated with the new changelist when deactivated and restore the context when the changelist becomes active, select the Track context option (refer to tasks and contexts for details) . 

If you are applying a patch from the clipboard, you can select the Analyze and apply patch from clipboard on the fly option for IntelliJ IDEA to automatically detect patches in clipboard contents and suggest applying them. 

If you want to move the patch to a temporary storage (shelf) before applying it, click Import to shelf (for more information, refer to Shelve or stash changes ). Otherwise, click OK . 07 August 2026 Shelve or stash changes Undo changes in Git repository
