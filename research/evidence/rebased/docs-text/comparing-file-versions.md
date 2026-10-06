<!-- source: https://www.jetbrains.com/help/idea/comparing-file-versions.html, fetched 2026-10-05 -->
Compare file and folder versions using Diff viewer | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Compare file and folder versions using Diff viewer 

IntelliJ IDEA allows you to examine the differences between two revisions of a file/folder or between their current local copy and the repository version. The differences are displayed in the Diff Viewer . 
For more information about filtering, navigating and applying changes in the Diff Viewer , refer to Compare files, folders, and text sources . 

### Compare a modified file with its Git repository version 

Open the Commit tool window ( Alt+0 ) . 

Locate the required file in a changelist and do one of the following: 

Right-click the file and select Git | Show Diff . 

Select the file and press Ctrl+D . 

Double-click the file. 

The diff view opens where changes to the file are highlighted. 
The right pane contains the modified version of the file. You can edit it in the diff view. 
The left pane contains the initial version of the file. It is read-only. You can click ( Revert ) to undo a change. 
If you hover over the commit hash on top of the left pane, a tooltip will show the commit details, such as commit message, author, date and time, and full commit hash. You can click the full commit hash to open the corresponding commit in the Log tab of the Git tool window Alt+9 . 

### Compare the current revision of a file or folder with a revision in the same Git branch 

Select a file or a folder in the Project tool window ( Alt+1 ) , and choose Git | Compare With Revision from the context menu. 

Choose a revision you want to compare the current file or folder version with from the dialog that opens. 

### Compare the current revision of a file or folder with another Git branch or tag 

Select a file or a folder in the Project tool window ( Alt+1 ) , and choose Git | Compare With Branch or Tag from the context menu. 

Choose the branch or tag you want to compare the current file or folder version with from the dialog that opens. 

## Compare local changes with the base revision 

Apart from navigating through your local changes within a file in the editor, you can review these changes compared to the base revision of the file. 
To preview the diff, select a modified file in the Commit tool window and double-click it or press Ctrl+D . 
The left pane shows the affected code as it was in the base revision, and the right page shows the affected code after you've made changes locally. 
Use the toolbar buttons and controls to navigate between changes and configure the appearance of the Change Details pane or the Diff Viewer : 
 | 
Item | 
Tooltip and Shortcut | 
Description 
 | 
/ | 
Previous Difference / Next Difference Shift+F7 F7 | 
Jump to the next or previous difference. 
When the last or the first difference is reached, IntelliJ IDEA suggests clicking the arrow buttons or pressing F7 / Shift+F7 once more and comparing other files modified locally. This behavior depends on the Go to the next file after reaching last change option in the Diff Viewer settings . 
 | 
 | 
Jump to Source 
F4 | 
Open the selected file in the editor. The caret is placed in the same position as in the Diff Viewer . 
 | 

 | 
Compare Previous/Next File 
Alt+Left Alt+Right | 
Compare the local copy of the previous or next file with its update from the server. 
These controls are only available if more than one file has been modified locally. 
 | 
Side-by-side viewer / Unified viewer | 
Select a viewer mode: side-by-side or unified. The side-by-side mode has two panels, and the unified mode has one panel. 
You can edit code and perform the Accept , Append , Revert actions in both viewers. 
You can change text only in the right part of the side-by-side viewer or in the lower line in the unified viewer. 
You can edit only local versions of your files. You cannot edit files that have read-only status. 
 | 
 | 
Collapse Unchanged Fragments | 
Collapse all the unchanged fragments in both files. The number of non-collapsible unchanged lines is configurable on the Diff & Merge settings page. To open the Diff & Merge page, open settings by pressing Ctrl+Alt+S and navigate to Tools | Diff & Merge . . 
 | 
 | 
Settings | 
Some of these commands are also available from the context menu of the Diff Viewer gutter. 
Align Changes in Side-by-Side Diff : vertically aligns the corresponding lines by padding differences with empty space. 
Synchronize Scrolling: scroll both diff panes simultaneously. 
Show Diff in Editor Tab : displays the Diff viewer as an editor tab instead of a separate window. 
Show Diff in Separate Window : displays the Diff viewer as a separate window instead of an editor tab. 
Ignore Differences : define how the Diff Viewer should treat whitespaces. 

None : white spaces are important, and all the differences are highlighted. This option is selected by default. 

Trim whitespaces : trim whitespaces if they appear in the end and at the beginning of a line ( ("\t", " ") ). 

If two lines differ in trailing whitespaces only, these lines are considered equal. 

If two lines are different, trailing whitespaces are not highlighted in the By word mode. 

Ignore whitespaces : white spaces are not important, regardless of their location in the source code. 

Ignore whitespaces and empty lines : ignores whitespaces and empty lines. The following entities are ignored: 

all whitespaces (as in the 'Ignore whitespaces' option) 

all added or removed lines consisting of whitespaces only 

all changes that consist of splitting or joining lines without changes to non-whitespace parts. 
For example, differences between a b c and a \n b c are not highlighted in this mode. 

Ignore imports and formatting : changes within import statements and whitespaces are ignored (whitespaces within String literals are respected though). 
Highlighting Differences : select the way differences granularity is highlighted. 

Words : modified words are highlighted 

Lines : modified lines are highlighted 

Split changes : if this option is selected, big changes are split into smaller changes. 
For example, A \n B and A X \n B X are treated as two changes instead of one. 

Characters : modified symbols are highlighted 

None : if this option is selected, the differences are not highlighted at all. 
Use the None option when you work with the files that were significantly modified. In such cases, highlighting might introduce additional difficulties during a review. 
 | 
 | 
Show diff in external tool | 
Invoke an external diff viewer specified on the External Diff Tools settings page. 
This button is available only on the toolbar when the Use external diff tool option is enabled on the External Diff Tools settings page. 
 | 
 | 
Help 
F1 | 
Open a browser and show the corresponding help page. 
 | | 
Annotate with GitBlame | 
This option is only available from the context menu of the gutter. 
Use this option to explore who introduced which changes to the repository version of the file and when. The annotations view lets you see detailed information for each line of code, such as the version from which this line originated, the ID of the user who committed this line, and the commit date. 
For more information about annotations, refer to Locate code author (Annotate with Git Blame) . 
The most useful shortcuts are the following: 
 | 
Shortcut | 
Description 
 | 
Ctrl+Shift+D | 
Use this keyboard shortcut to show the popup menu of the most commonly used diff commands. 
 | 
Ctrl+Shift+Tab | 
Use this keyboard shortcut to switch between the left and the right panes. 
 | 
Ctrl+Z / Ctrl+Shift+Z | 
Use this keyboard shortcut to undo/redo a merge operation. Conflicts will be kept in sync with the text. 17 August 2026 Sign commits with GPG keys Investigate changes in Git repository
