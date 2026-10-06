<!-- source: https://www.jetbrains.com/help/idea/advanced-settings.html, fetched 2026-10-05 -->
Advanced Settings | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Advanced Settings 

Besides standard settings that you frequently use, IntelliJ IDEA offers various advanced options that you can use to fine-tune the behavior of the specific product functionality. 
Since the advanced settings consist of different sections, you can use the search field to quickly navigate to the section of interest or the necessary option. You can also search for registry keys since some of the advanced options have associated registry keys. 

## Angular 

 | 
Item | 
Description 
 | 
Show navigation popup in editor | 
When this checkbox is selected, IntelliJ IDEA displays a navigation popup with which you can quickly jump between files within an Angular component, navigate to styles and templates within a file or open a popup with a list of related files. 

## Appearance 

 | 
Item | 
Description 
 | 
Enable Islands UI for custom themes | 
Enable the Islands UI elements, such as rounded corners and section separation, for custom UI themes installed from JetBrains Marketplace. By default, Islands UI is applied only to the built-in Islands themes . 

## Bookmarks 

 | 
Item | 
Description 
 | 
Show only line bookmarks in popup | 
By default, IntelliJ IDEA shows only line bookmarks in the bookmarks popup ( Shift+F11 or Edit | Bookmarks | Show Line Bookmarks… ). Disable this option if you want to see other bookmarked items in the popup, for example, files or folders. For more information, refer to Bookmarks . 
 | 
Restore bookmark context on branch switching | 
By default, IntelliJ IDEA maintains a separate bookmark context for each VCS branch. Switching branches restores the bookmarks that were set for that branch. Disable this option to preserve bookmarks across branch switches whenever possible. For more information, refer to Preserve bookmarks when switching branches . 

## Build Tools. Gradle 

 | 
Item | 
Description 
 | 
Run using Gradle | 
This option is selected by default. The build and run actions will be executed by Gradle. If you clear this option, then IntelliJ IDEA will build and run your project. 
 | 
Download sources | 
By default, IntelliJ IDEA doesn't download sources for the project dependencies on the initial project sync. 
Select this option to change the project's behaviour and always download sources for dependencies during the importing process. 
 | 
Attach scripts dependencies sources | 
This option attaches source JARs for your build script dependencies (namely the classpath entries declared in the buildscript { dependencies { ... } } block of your build.gradle files) during Gradle project import. 
When this option is enabled, the IDE will download and attach source artifacts for the plugins and libraries used by the build scripts themselves — not your project's runtime/compile dependencies, but the Gradle plugins and tooling on which your build scripts depend. This lets you navigate into their source code from within the IDE. A project sync is required after enabling it for the sources to be attached. 

## Code Completion 

 | 
Item | 
Description 
 | 
Enable automatic completion on typing | 
Display completion suggestions automatically while you type. If disabled, suggestions are shown only when you press Alt+Shift+\ . 

## Code Review Tools 

 | 
Item | 
Description 
 | 
All-in-One Diff for Code Reviews | 
Select this option to work with all changed files in code reviews in a single scrollable Diff view. 
 | 
Keep increased margins for Diff threads | 
Clear this option to reduce the horizontal space around the comments in code reviews. 

## Compiler 

Use this section to configure additional options when compiling a project . 
 | 
Item | 
Description 
 | 
Allow auto-make to start even if developed application is currently running | 
Select this option to automatically build your project even if the application you are working on is currently running. Note that some application classes might be deleted during the build. 
 | 
Run compilation with lower priority | 
Use this option to lower the compilation process priority to keep the user's machine responsive during compilation. Keep in mind that it affects the compilation duration. 
 | 
Enable unified Java/Kotlin incremental compilation implementation | 
This option is enabled by default. The external JPS build process uses the newer incremental compilation implementation, which tracks both Java and Kotlin bytecode together. 
 | 
Collect a detailed debug log report in case of build failures | 
Enable this option for the IntelliJ IDEA build process to keep a detailed log of each build in memory and only persists it to disk if that build fails. This diagnostic feature helps troubleshoot build failures without the overhead of writing verbose logs for successful builds. 

## Container Environments WSL, Docker 

 | 
Item | 
Description 
 | 
Shell mode for fetching environment variables | 
Use this option to control how the IDE launches a shell to read environment variables for container and remote environments, such as WSL and Docker. You can choose whether the shell is started in interactive mode (default) or non-interactive mode. This determines which shell startup files are executed and, consequently, which environment variables are available to the IDE. 

## Database 

 | 
Item | 
Description 
 | 
Open file as table if detected by scripted loader | 
For the files of formats supported by scripted data loaders . Defines which files IntelliJ IDEA opens in a table view. 

All : All supported tabular data files. 

Table-first formats : Files of the file formats dedicated to representing data in a table format. Currently, all supported tabular data file formats, except JSON. 

None : Disables the view for all files. 
 | 
Related Rows action behavior | 
Defines what the Related Rows action does when invoked in the table cell's floating toolbar or context menu. 

Navigate And Filter All : Shows only the referenced rows. 

Navigate And Select First : Shows all the rows in the corresponding table, and the first of the referenced rows is selected. 
 | 
Show Database tab in Search Everywhere | 
Enables the Database tab in Search Everywhere (double Shift ). 
For more information about the Search everywhere feature, refer to Search everywhere . 

## Debugger 

 | 
Item | 
Description 
 | 
Maximum number of recent expressions | 
The number of expressions to be saved in the Evaluate expression history. 
 | 
Show inlay Run to Cursor popup | 
Controls whether the Run to Cursor icon should appear when you hover over a line in debug mode. Clicking this icon continues the execution until the corresponding line is reached. 
 | 
Highlight current element in browser on caret change | 
Use this checkbox to configure highlighting in the browser during a debugging session. 

By default, the checkbox is selected and an element is highlighted in the browser when the caret moves to the corresponding code fragment in the editor. 

When the checkbox is cleared, hold Shift and hover over the element in the editor to highlight it in the browser. 

## Dev Containers 

 | 
Item | 
Description 
 | 
Open devcontainer projects natively | 
Use this option to start a Dev Container and open a project in it as you would a local one. No additional windows will be opened. 
For IntelliJ IDEA this option is selected by default. 
Note the following: 

This feature is not supported in the IntelliJ IDEA versions earlier than 2025.3. 

Only Docker for Dev Containers is supported. 

Starting Dev Container for a remote project is not supported with this option. 
For more information, refer to Open a project with Dev Container natively . 
 | 
Clone sources with --depth=1 | 
Use this option to speed up your project cloning process by checking out the latest version of the source code. For more information, refer to Git documentation . 
 | 
Choose the backend version automatically | 
When this option is selected, the latest version of the IDE will be selected automatically. 
 | 
Display notification if deployment of dev container backend/frontend takes more than 10 seconds | 
When this option is selected, the IDE displays a notification. 
 | 
Use '--no-recreate' when rebuild Docker Compose Dev Container. The Main service will always be recreated. | 
When this option is selected, the primary development container will be recreated even if you use the --no-recreate option to avoid rebuilding unchanged containers. 

## Docker 

 | 
Item | 
Description 
 | 
Connect to Docker automatically at restart | 
Connect to the configured Docker daemon after IntelliJ IDEA starts. 
 | 
Image registry entities loading limit | 
For performance reasons, by default, IntelliJ IDEA shows only the first 100 images in a Docker registry . Increase this number if you need to list more. This will also increase the memory usage. 
 | 
The log tab should be the first tab for the docker container. | 
Show the Log tab as the first tab in the container overview pane . 
 | 
The delay between periodic Docker status checks for automatic reconnection | 
How often should IntelliJ IDEA check the status of connection to Docker in milliseconds. 
 | 
Apply :z mount option to almost any bind volume for SELinux systems | 
Add :z configuration flags to Docker bind mounts to connect volumes to Docker containers on SELinux workstations. 

## Documentation Components 

This feature works for Java and Kotlin. 
Enable highlighting for code elements in documentation components ( Quick Documentation popup and tool window and rendered Javadocs ). 
 | 
Item | 
Description 
 | 
Basic syntax highlighting of inline code | 
Highlight keywords, numbers, and strings in all inline code elements in documentation components. 
 | 
Basic syntax highlighting of multiline code blocks | 
Highlight keywords, numbers, and strings in all multiline code blocks in documentation components. 
 | 
Syntax highlighting of links on code elements | 
All links in code elements will be fully highlighted with the color corresponding to the color that is selected in the editor color scheme of this element (class, method, field, and so on). 

## Editor 

Use this section to configure additional options for working in the editor . 
 | 
Item | 
Description 
 | 
Use undo transparent caret movement | 
Select this option if you want to revert your last edit immediately as in regular text editor. 
 | 
Don't copy/cut the current line when invoking the Copy or Cut action with no selection | 
This option lets you override the default behavior of copying Ctrl+C or cutting Ctrl+X the whole current line when nothing is selected in the editor. 
 | 
Don't select the copied line after invoking the Copy action with no selection | 
By default, when you use Ctrl+C without selecting anything, the whole line gets copied. With this action, you can avoid that selection. 
 | 
When pasting a line copied with no selection | 
You can change the default Paste above the caret line behavior, and select either Paste at the caret line that restores the old Copy / Paste behavior or Trim if pasted middle line that will remove the unnecessary spaces before and after the copied line if it is pasted in the middle of the line. 
 | 
Render special characters, such as control codes, using their Unicode name abbreviations | 
When this option is selected, you can use the Unicode name abbreviations for special characters, such as control codes. 
 | 
Show zoom indicator | 
Use this option to see an indicator that shows the current font size and the option to revert it back to default while zooming your code in or out in the editor. 
 | 
Tab character rendering | 
You can select one of the following options: 

Horizontal line 

> symbol 

Long arrow 
 | 
Left margin in distraction free mode | 
This option lets you adjust the left editor margin in the distraction-free and zen viewing modes . 
By default, the editor text in these viewing modes is centered vertically and the text column width corresponds to the Hard wrap at option on the Editor | Code Style settings page Ctrl+Alt+S . This means that if your code will be centered when formatted properly , but if there are long unwrapped lines, the text might go beyond the viewport while there is a large margin on the left. In such cases, you may want to decrease the left margin using this option. 
 | 
Line number's font delta | 
Use this option to change the font size of the line numbers in the left gutter comparing to the font size in the editor. 
 | 
Force soft wrap in documents with lines longer than n characters | 
The default number is 100000 . Increasing this number might negatively affect the editor performance. 
 | 
Select whitespace characters with 'Extend Selection' | 
Use this option to include whitespaces when you invoke the Extend Selection action. 
 | 
Move caret down after Comment with Line Comment action | 
This option lets you configure whether to move the caret to the next line if you comment a line with a line comment Ctrl+/ . Note that if there is a selection in the editor, the caret will not move regardless of this option. 
 | 
Hide floating toolbar for code editing | 
Use this option if you don't want IntelliJ IDEA to automatically display a floating toolbar when you edit your code. 

## Editor Tabs 

Besides the standard editor tabs options that IntelliJ IDEA offers when you work with tabs in the editor, this section contains extra options that are also available as internal registry actions. 
 | 
Item | 
Description 
 | 
When navigating to a file, prefer selecting existing tab in inactive split pane | 
When you navigate to a file that is already opened in the inactive split, use this option to activate the existing file in that split pane instead of opening a new copy of the file in the active split. 
 | 
Open declaration source in the same tab | 
When you navigate to a method, class or variable, the source file that contains their declaration will replace the current tab if there are no changes. 
 | 
Open declaration source called from a detached window in the main IDE window | 
When you navigate to a method, class, or variable from a detached window, IntelliJ IDEA will open the source file containing the declaration in the main editor. 
 | 
Perform 'Hide All Tool Windows' / 'Restore Windows' with double-click on editor tab | 
When this option is enabled, you can double-click editor tabs to hide and restore all tool windows . Alternatively, you can do that by pressing Ctrl+Shift+F12 or by choosing Window | Active Tool Window | Hide All Windows/Restore Windows from the main menu. 
 | 
Perform 'Maximize Editor' / 'Normalize Splits' with double-click on editor tab | 
When this option is enabled, you can double-click tabs in split editor tab groups to change relative size of the tab group: the first double click will extend the tab and its group to the maximum and shrink all other groups to the minimum width/height; the second double click will allocate the same width and height for each tab group. 
Alternatively, you can choose Window | Editor Tabs | Maximize Editor/Normalize Splits from the main menu or pick these actions from the Find Action popup Ctrl+Shift+A . 
This option can work together with Perform 'Hide All Tool Windows' / 'Restore Windows' with double-click on editor tab , but be prepared to have multiple changes in the IDE layout every time you double-click a fie tab. 
 | 
Equalize proportions in nested splits | 
Use this option to even the size of multiple horizontal or vertical splits in the editor. 
 | 
Keep pinned tabs on the left | 
Always move pinned tabs to the left side of the editor tabs bar. Otherwise, the location of pinned tabs is not defined, and you can move them on the tab bar as necessary. 

## Find / Replace 

Use this section to configure extra options when working with Search and replace a target within a project . 
 | 
Item | 
Description 
 | 
Enable similar usages clustering in Find Usages view | 
IntelliJ IDEA analyzes search results, detects the most common usage patterns, and categorizes all found usages into groups based on their structural similarity. These usage clusters appear in the Preview tab. 
For more information, refer to Search for usages in a project . 
 | 
Maximum number of results to show in Find in Files/Show Usages preview | 
The default maximum number of results is 100 . Increasing this limit might significantly increase memory usage. 
 | 
Limit at which the "Too many results found" warning is shown | 
The default limit is 1000 results. 

## Frameworks. Micronaut 

 | 
Item | 
Description 
 | 
Run using Gradle | 
By default, in Micronaut Gradle-based applications, IntelliJ IDEA uses Gradle to build the project and IntelliJ IDEA to run it. Select this checkbox to use Gradle instead of IntelliJ IDEA to run Micronaut applications. 

## Frameworks. Spring Boot 

 | 
Item | 
Description 
 | 
Run using Gradle | 
By default, in Spring Boot Gradle-based applications, IntelliJ IDEA uses Gradle to build the project and IntelliJ IDEA to run it. Select this checkbox to use Gradle instead of IntelliJ IDEA to run Spring Boot applications. 

## Gateway WSL 

 | 
Item | 
Description 
 | 
Open WSL projects natively as local projects (without Remote Development) | 
Use this option to open a project in the WSL file system as you would a local project. No additional windows will be opened. 

## GitHub 

 | 
Item | 
Description 
 | 
GitHub host aliases | 
Set aliases for github.com . 
Available only with the Ultimate subscription. 

## GitHub actions 

 | 
Item | 
Description 
 | 
GitHub actions authoring support (requires IDE restart) | 
Enables the GitHub Actions support that lets you automate your build, test, and deployment pipeline. 
Available only with the Ultimate subscription. 

## IDE 

Use this section to configure extra options on storing different entities of your project. 
 | 
Item | 
Description 
 | 
Maximum number of recent projects | 
Use this option to change the default number of stored recent projects, which is 50 . 
 | 
Maximum number of recent files | 
Use this option to change the default number of stored recent files , which is 50 . 
 | 
Maximum number of recent locations | 
Use this option to change the default number of stored recent locations , which is 25 . 
 | 
Duration of storing changes in Local History | 
Use this option to specify the number of days for which you want to keep changes in the local history . 

## JavaScript and TypeScript 

 | 
Item | 
Description 
 | 
Semantic highlighting accuracy | 
Use this field to configure how detailed you want semantic differences to be pointed out by highlighting. The available options are High , Normal (default), Basic , and Limited . 
Note that choosing a higher accuracy level may result in higher CPU usage and thus cause performance problems. 
 | 
Advanced JavaScript annotator | 
Select this checkbox to have IntelliJ IDEA perform deeper JavaScript-specific semantic analysis in JavaScript and JSX code. 
 | 
Highlight JSX client components | 
Select this checkbox to have IntelliJ IDEA apply different highlighting styles to JSX server and JSX client components. 
 | 
Advanced JSX annotator | 
Select this checkbox to have IntelliJ IDEA perform deeper semantic analysis of JSX tags in JSX and TSX code. 
 | 
Search in library files when 'Directory' is selected in Find in Files | 
Select this checkbox to have library files included in searching inside a specific folder. 
 | 
Fallback to IDE type evaluator for files outside the current import graph | 
Select this checkbox to suppress evaluating types from files outside the import graph that is determined by the project tsconfig.json file. 

## JVM Languages 

 | 
Item | 
Description 
 | 
Process terminal output to find class names and highlight them | 
IntelliJ IDEA recognizes messages from common logging frameworks, such as slf4j and log4j2 and allows you to navigate to the corresponding logger calls from the console. 
Use this option to show or hide the logger calls highlighting. 

## Java 

 | 
Item | 
Description 
 | 
Code Vision: Minimum usages required to show inlay hints | 
Configure the minimum number of usages for displaying inlay hints . 
 | 
Enable completion for static methods, which uses qualifier as a first argument | 
When the target class is the first parameter, static methods are treated the same as instance methods. This makes relevant completion suggestions more discoverable. To display the necessary suggestion, invoke code completion ( Ctrl+Space ) twice. 
 | 
Configure JDK with external tool files on project opening ('.sdkmanrc', '.tool-versions', …) | 
Synchronize the use of JDK declarations specified in the sdkman configuration in the project. 
 | 
Show irrelevant New File templates in Java source roots | 
Display all non-essential actions from the New context menu that you can open by right-clicking a node inside the Java sources root folder . 

## Java Bytecode Decompiler 

 | 
Item | 
Description 
 | 
Max count of direct nodes | 
Specify how complex a single method is allowed to be for decompilation. 
If you increase the value, the IDE will try to fully decompile larger, more complex methods at the cost of time and memory. If you decrease it, complex methods might be cut off early, but the decompiler will stay faster. 
 | 
Max count of variable nodes | 
Specify how many variable nodes the decompiler will build and process for a single method during decompilation. 
If a method is very complex, increasing this limit may improve decompilation but can reduce performance and increase memory usage. Lower values make the decompiler stop earlier on complex methods. 

## Kotlin 

 | 
Item | 
Description 
 | 
Compiler reference index | 
Enable this option to search for usages with the help of references from compiler indexes. 

## Kubernetes 

 | 
Item | 
Description 
 | 
Show an empty Kubernetes node if no cluster available | 
If selected, IntelliJ IDEA displays the Kubernetes node in the Services tool window if no clusters are available. 
 | 
Maximum size of kubeconfig file to load | 
Specify the maximum size, in megabytes (MB), of a kubeconfig file that IntelliJ IDEA will attempt to load. 
 | 
Maximum size of CRD YAML file to load | 
Specify the maximum size, in megabytes (MB), of a resource YAML file that IntelliJ IDEA will attempt to load. 
 | 
Timeout for kubectl cluster operations | 
Specify the timeout in seconds for IntelliJ IDEA to wait for kubectl cluster operations to complete. If an operation takes longer, it is canceled due to delays or no response. 
 | 
Timeout for HTTP requests | 
Specify the timeout in seconds for IntelliJ IDEA to wait for HTTP requests sent to the Kubernetes cluster. If a request takes longer, it is canceled due to delays or no response. 

## Markdown 

 | 
Item | 
Description 
 | 
Hide floating toolbar | 
When you select some text in a Markdown file, IntelliJ IDEA shows a floating toolbar with various formatting options. Select this checkbox if you don't want to see the floating toolbar, then reopen the tab to apply the changes. 
 | 
Squash multiple dashes in header anchors | 
Convert two or more dashes to one in anchors generated from chapter headers. 

## Other 

 | 
Item | 
Description 
 | 
Maximum amount of requests in HTTP Client requests log | 
Specify how many executed HTTP requests will be stored in the request history in http-requests-log.http . 
When the number of stored requests exceeds the specified limit, the oldest requests and their associated response files are removed automatically. 

## Project View 

Use this section to configure additional options for working in Project tool window . 
 | 
Item | 
Description 
 | 
Move focus to editor when Enter is pressed | 
When this option is selected, you can press Enter while in the Project view to switch focus to the editor. 
 | 
When collapsing a node, also collapse all expanded nodes under it | 
When this option is selected, IntelliJ IDEA collapses subnodes as well as the parent node. 
 | 
When navigating to a library class, do not select it in the project tree | 
Do not automatically navigate to library files in the Project tool window even if the Always Select Opened File option is enabled. 
You can navigate to library files in the tool window manually: use the Select Opened File button on the Project tool window toolbar or press Alt+F1 to open the Select In popup. 
 | 
Increase font size in Project view | 
Select this option if you want to change the size of the font in the Project view. You need to restart IntelliJ IDEA. 
 | 
Enable Scratches and Consoles in the Project View | 
Show or hide the Scratches and Consoles node in the Project tool window on the IDE level. 
This node contains your scratch files and query consoles . 
To hide the node per project, use the corresponding option in the tool window settings . 

## Rails 

 | 
Item | 
Description 
 | 
Load generators automatically | 
Enabled by default, use this option to automatically load Rails generators. When the option is disabled, you can still trigger the action manually. 

## Rake 

 | 
Item | 
Description 
 | 
Load tasks automatically | 
Enabled by default, use this option to automatically load Rake tasks. When the option is disabled, you can still trigger the action manually. 

## Run/debug 

 | 
Item | 
Description 
 | 
Temporary configurations limit | 
Use this option to change the default number of stored temporary run/debug configurations , which is 5 . 
 | 
Confirm rerun with process termination | 
When you call rerun for a running process, IntelliJ IDEA prompts you to confirm the action of terminating the existing process and launching a new one. You can change the default behavior by clearing this checkbox. 
 | 
Make configurations pinned by default | 
Select this option to automatically pin tabs that open in the Run tool window after you run configurations. 
 | 
Maximum number of recent run configurations in the new UI | 
Specify the number of recent run/debug configurations that will be displayed in the Run widget . 

## SSH 

 | 
Item | 
Description 
 | 
Configuration files parser | 
Select a parser: 

Legacy : use the legacy parser for ~/.ssh/config . 

OpenSSH : use the parser based on ssh -G . OpenSSH must be installed on your computer. 
 | 
Custom path to an OpenSSH tool | 
Specify the path to OpenSSH if you have selected the OpenSSH parser. 

## Search Everywhere 

 | 
Item | 
Description 
 | 
Show text search results in Search Everywhere | 
Enable text search in Search Everywhere . Results will be displayed on the new Text tab and on the All tab at the bottom of the list. 
 | 
Wait for all contributors to finish before showing results | 
Wait for all search contributors to finish their jobs before showing the final search results. This option fixes the position of an item in search results, preventing it from jumping in the list. In case there are slow search contributors, this option might slow down the operation of Search Everywhere. 
In the Contributors waiting timeout (ms) field, specify how much time to wait for search contributors to finish their jobs. 
 | 
Show notifications above search results | 
This option is enabled by default. The informational notifications such as hints are displayed as non-selectable gray information rows among the search results. When this option is disabled, the Search Everywhere window displays only the actual search results. 
 | 
Contributors waiting timeout (ms) | 
When you search, results are returned from multiple sources at different speeds. To avoid a constantly reshuffling or flickering results list, enabling this option briefly delays displaying the results so they can be collected and shown together. This value specifies the maximum delay (2,000 ms, or 2 seconds, by default) before the results are displayed, even if some slower sources have not finished. If the primary sources return enough matches sooner, the results are displayed immediately. The timeout only applies when the primary sources are slow. 
 | 
Show recent files at the top of results list | 
Enable this options to display the results from the recent files first instead of ranking them by how well they match your query. 
 | 
Enable typo-tolerant search in the Search Everywhere Action tab Support | 
When you search for an IDE action ( Ctrl+Shift+A ) and misspell its name, the IDE recognizes the intended spelling and still finds the matching action. For example, entering a misspelled action name returns the correct action instead of no results. 
 | 
Sort results in the Actions tab based on machine learning | 
By default, the search results on the Actions tab are sorted using machine learning instead of the standard ranking mechanism. 
 | 
Sort results in the Files tab based on machine learning | 
By default, the search results on the Files tab are sorted using machine learning instead of the standard ranking mechanism. 
 | 
Sort results in the Classes tab based on machine learning | 
By default, the search results on the Classes tab are sorted using machine learning instead of the standard ranking mechanism. 
 | 
Sort results in the All tab based on machine learning | 
Sort search results on the All tab using machine learning instead of standard ranking mechanism. 
 | 
Enable semantic search in the Actions tab | 
Select this option if you want IntelliJ IDEA to use semantic search (a technique that analyzes the intent and contextual meaning behind a search query) in the Actions tab, delivering more relevant results. 
 | 
Enable semantic search in the Files tab | 
Select this option if you want IntelliJ IDEA to use semantic search (a technique that analyzes the intent and contextual meaning behind a search query) in the Files tab, delivering more relevant results. 
 | 
Enable semantic search in the Symbols tab | 
Select this option if you want IntelliJ IDEA to use semantic search (a technique that analyzes the intent and contextual meaning behind a search query) in the Symbols tab, delivering more relevant results. 
 | 
Enable semantic search in the Classes tab | 
Select this option if you want IntelliJ IDEA to use semantic search (a technique that analyzes the intent and contextual meaning behind a search query) in the Classes tab, delivering more relevant results. 

## Search Scope in Find, Replace, Rename 

 | 
Item | 
Description 
 | 
Keep last selected search scope | 
Select this option to quickly look up the history of the search scope. 

## Security Analysis 

 | 
Item | 
Description 
 | 
Show Problem Tab | 
Use this option to display Java or Kotlin security code analysis output. 

## Startup 

 | 
Item | 
Description 
 | 
Open README.md file if there are no open files on project startup | 
If you select this option, IntelliJ IDEA will open a README.md file if there aren't any opened files when you start your project. 

## Suggested refactoring 

 | 
Item | 
Description 
 | 
Show a hint for suggested refactoring in the editor | 
Show inline hints in the editor when a refactoring is detected. When the option is disabled, refactorings remain available, but the hints are not shown. 

## Terminal 

Configure advanced settings related to working with the embedded terminal emulator . 
 | 
Item | 
Description 
 | 
Show application title | 
Show the title set by the application running in the terminal. 
 | 
Terminal scrollback buffer size | 
Use this option to increase the buffer size in the terminal. 
 | 
Typeahead | 
Enable typeahead in the terminal, which can be useful for remote connections when you expect the typing latency. 
 | 
Typeahead latency threshold | 
Turn typeahead on when the terminal latency exceeds the specified value (in milliseconds). 
 | 
Use 1.0 line spacing for alternative screen buffer | 
Terminal applications that use alternate screen buffers may have their own visual style. This setting maintains using 1.0 line-spacing in the alternate screen buffer mode of the terminal. 
 | 
Fill character background including line spacing | 
If the line spacing value in the alternate screen buffer mode exceeds 1.0, white horizontal gaps might appear between lines. Select this checkbox to fill in these gaps. 
 | 
Terminal character encoding | 
Specify encoding for the terminal. 
 | 
New terminal output capacity | 
Specify the buffer size for the Reworked terminal . 
 | 
Send Esc+CR on Shift+Enter | 
Select this option to send Esc + Carriage Return to the shell when pressing Shift Enter , instead of a regular carriage return. 
By default, the terminal sends the same key sequence for Enter and Shift Enter . When this option is enabled, pressing Shift Enter sends an additional Esc character before the carriage return. This allows command-line applications to distinguish and handle Shift Enter differently, for example, by inserting a new line when entering multi-line commands. 
 | 
Start SSH terminal in the deployment directory | 
Select this option to start new SSH sessions in the deployment directory. 
If disabled, SSH sessions start in the default remote directory (home or the path defined in the SSH configuration). 

## Tool Windows 

 | 
Item | 
Description 
 | 
Always show tool window header icons | 
By default, the icons in tool window headers (for example, and ) are displayed only when you hover over a tool window or when a tool window is in focus. 
Enable the option to display tool window header icons all the time. 
 | 
Allow dragging tool windows by header to move to another location | 
Move tool windows by dragging the header. 
When the option is disabled, you can move tool windows by dragging tool window bars, using the Move to context menu option or the tool window settings located under the / icon. 

## User Interface 

 | 
Item | 
Description 
 | 
Use native file chooser dialog on Windows/macOS | 
On Windows and macOS, the IDE will use an OS-specific file chooser dialog for opening files and directories. 
 | 
Show file type icon in IDE frame header | 
On macOS, the icon can be used to drag a file to a different application. 
 | 
Use words instead of symbols for macOS keyboard shortcuts | 
Select this option if you want to see shortcuts with words instead of symbols. 
 | 
Activate menu items on right mouse button release | 
When this option is enabled (the default), releasing the button over an item selects it, and releasing anywhere else closes the menu. When it is disabled, this hold-and-release gesture does nothing — you open the menu and click items the usual way. 
This option is supported for Linux and macOS only. 
 | 
Cyclic scrolling in lists and trees | 
Select the last element when pressing Up on the first element, and vice versa. 
 | 
Position mouse cursor on default button in dialogs | 
When this option is selected, the mouse pointer is placed on the default button (usually it is OK ) in a dialog. 
 | 
Disable double modifier key shortcuts | 
You can disable shortcuts where a double key is used. For example, Shift or Ctrl . 
 | 
Show settings in a non-modal dialog | 
By default, the settings window is non-modal, which means you can interact with the IDE while the settings window is open. 
Clear this option to make the settings window modal and block interaction with the IDE until it is closed. 

## Version Control 

 | 
Item | 
Description 
 | 
Open Diff as Editor Tab | 
When this option is selected, the Diff Viewer is displayed as the editor tab. 
 | 
All-in-One Diff | 
When this option is selected, all changed files are shown in a single, continuous Diff view. 
 | 
Load file annotations from VCS when file is opened in editor | 
When this option is selected, IntelliJ IDEA loads annotations when you open a file in the editor. 
 | 
Highlight ignored files | 
When this option is selected, IntelliJ IDEA shows the list of ignored files from VCS and displays their status in the editor. 
 | 
Enable Commit tool window | 
When you select this option, IntelliJ IDEA displays the Local Changes and Shelf tabs in a separate tool window. 
 | 
Toggle commit controls | 
When this option is selected, IntelliJ IDEA hides the commit panel and checkboxes after the commit is performed and lets you toggle them in the Commit Changes dialog. 
 | 
Automatically close the Commit tool window in Window/Float mode after committing | 
This option is available when the Commit tool window is in Window or Float mode . When selected, the tool window closes automatically after the changes are committed. 
 | 
Select all repositories with new commits for push | 
When this option is selected, IntelliJ IDEA selects all commits that will be pushed by default. 
 | 
Do not show a warning when force pushing | 
This option applies to force push operations on unprotected branches. When selected, IntelliJ IDEA will not ask for confirmation before force pushing. 
 | 
Use modal commit interface for Git and Mercurial | 
Select this option to commit local changes in a separate modal commit dialog. 

## Version Control. Git 

 | 
Item | 
Description 
 | 
Recursively clone submodules in the project | 
When this option is selected, during the checkout from Git, git clone --recurse-submodules is used that clones the main repository with all available submodules. 
 | 
Apply content transformation when reading from Git | 
When this option is selected, IntelliJ IDEA will use the --filters or --textconv flags for the git cat-file command when reading the file content from Git. 
 | 
Use Safe Force Push | 
When this option is selected, IntelliJ IDEA uses --force-with-lease when force push is called from the IDE instead of just --force . 
 | 
Check for incoming and outgoing commits | 
When this option is selected, IntelliJ IDEA checks and indicates the incoming (blue arrow icon ) or outgoing (green arrow icon ) commits next to the branch name in the VCS widget. 
 | 
Automatically open the 'Update info' tab after updating the project | 
After performing the update operation, the Update info tab opens in the Git tool window Alt+9 . 
 | 
Do not run Git commit hooks | 
Select this option to disable running commit hooks automatically during commit operations. 
 | 
Branch name cleanup symbol | 
Specify a symbol to automatically replace spaces and other restricted characters in Git branch names (default: - ). 
 | 
Show branch names in the list of recent projects | 
By default, the name of the branch which is checked out in the project is shown in the Welcome screen and in the list of recent projects in the main menu. You can disable this setting or switch it only for the Projects with identical names . 
 | 
Show Git tab in Search Everywhere | 
Select this option to add the Git tab to the Search Everywhere window. 11 September 2026 Web Services Icon reference
