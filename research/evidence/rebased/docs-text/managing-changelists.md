<!-- source: https://www.jetbrains.com/help/idea/managing-changelists.html, fetched 2026-10-05 -->
Group changes into changelists | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Group changes into changelists 

A changelist is a set of local changes that have not yet been committed to a Git repository. 
With changelists, you can group changes related to different tasks and commit these sets of changes independently. For more information, refer to Commit changes locally . 
Changelists are just one of the ways to work on several features simultaneously . 
Changelists are displayed in the Commit window of the Commit tool window ( Alt+0 ) . Initially, there is a single default changelist called Changes . All new changes are automatically placed in the Changes changelist. There is also an Unversioned Files changelist that groups newly created files that haven't been added to Git yet. 
You can create as many changelists as needed and make any of them active at any moment. You can move any uncommitted changes to any changelist. 

### Create a new changelist 

In the Changes view, right-click a file to open the context menu and select New Changelist . 

In the New Changelist dialog, specify the name of the new changelist and add a description (optional). 

### Set the active changelist 

In the Changes view, select a non-active changelist and press Ctrl+Space or right-click it and choose Set Active Changelist from the context menu. All new changes will be automatically placed in this changelist. 

### Move changes between changelists 

In the Changes view, select the changes that you want to move to another changelist. 

Right-click the selection and choose Move to Another Changelist Alt+Shift+M . 

In the dialog that opens, select an existing changelist or enter the name for a new changelist. 
Type an optional comment. When the new changelist is submitted to the repository, this comment will appear in the Comment text area of the Commit Changes dialog. 

Select the Set active checkbox to set the active status for the new changelist right after the changes are restored in it. 
When this checkbox is cleared, the current active changelist remains active. 

Select the Track context checkbox to have IntelliJ IDEA preserve the context of the task associated with the new changelist on its deactivation and restore the context when the changelist becomes active. 
For more information, refer to Manage tasks . 
You can also drag files between changelists. 
For more information about placing changes within one file into different changelists in Git, refer to Put changes into different changelists . 

### Delete a changelist 

Right-click a changelist and choose Delete Changelist from the context menu. 17 August 2026 Resolve Git conflicts Shelve or stash changes
