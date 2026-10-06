<!-- source: https://www.jetbrains.com/help/idea/set-up-a-git-repository.html, fetched 2026-10-04 -->
Set up a Git repository | IntelliJ IDEA Documentation llms.txt : machine-readable documentation index. Markdown version of this page is also available. 

### IntelliJ IDEA 2026.2 Help 

# Set up a Git repository 

When you clone an existing Git repository or put an existing project under Git version control , IntelliJ IDEA automatically detects if Git is installed on your computer. If the IDE can't locate a Git executable, it suggests downloading it. 
IntelliJ IDEA supports Git from the Windows Subsystem for Linux 2 (WSL2), which is available in Windows 10 version 2004 . 
If Git is not installed on Windows, IntelliJ IDEA searches for Git in WSL and uses it from there. Also, IntelliJ IDEA automatically switches to Git from WSL for projects that are opened when you use the \\wsl$ path. 
If you need to manually configure IntelliJ IDEA to use Git from WSL, go to the Version Control | Git settings page Ctrl+Alt+S , click the Browse icon in the Path to Git executable field, and select Git from WSL via the \wsl$ path, for example, \\wsl$\debian\usr\bin\git . 

## Check out a project from a remote host (git clone) 

IntelliJ IDEA allows you to check out (in Git terms, clone ) an existing repository and create a new project based on the data you've downloaded. 

To start cloning a Git repository, do one of the following: 

If the version control integration is already enabled, go to Git | Clone . 

If the version control integration is not enabled yet, go to VCS | Get from Version Control . 
Alternatively, go to File | New | Project from Version Control . 

If no project is currently open, click Clone Repository on the Welcome screen. 

In the Clone Repository dialog, specify the URL of the remote repository you want to clone or select one of the VCS hosting services on the left. 
If you are already logged in to the selected hosting service, completion will suggest the list of available repositories that you can clone. 
If you want to perform shallow clone with a limited history, select the Shallow clone with a history truncated to checkbox and specify the number of commits that you want to clone. 
You can fetch the rest of the history later by selecting Git | Unshallow Repository in the main menu. 

Click Clone . If you want to create a project based on the sources you have cloned, click Yes in the confirmation dialog. Git root mapping will be automatically set to the project root directory. 
If your project contains submodules , they will also be cloned and automatically registered as project roots. 

In the Trust and Open Project '<project_name>'? project security dialog , select the way you want to open the project: Trust Project or Preview in Safe Mode . 

When you import or clone a project for the first time, IntelliJ IDEA analyzes it. If the IDE detects more than one configuration (for example, Eclipse and Gradle), it prompts you to select which configuration you want to use. 
If the project that you are importing uses a build tool, such as Maven or Gradle , we recommend that you select the build tool configuration. 
Select the necessary configuration and click OK . 
The IDE pre-configures the project according to your choice. For example, if you select Gradle , IntelliJ IDEA executes its build scripts, loads dependencies, and so on. 

## Put an existing project under Git version control 

You can create a local Git repository based on the existing project sources. 

### Associate the entire project with a single Git repository 

Open the project that you want to put under Git. 

Press Alt+` to open the VCS Operations Popup and select Enable Version Control Integration . 
Alternatively, go to VCS | Enable Version Control Integration . 

Choose Git as the version control system and click OK . 
The entire project will then be associated with a single Git directory. After that, add the files you want to be tracked by Git to the local repository. 

After VCS integration is enabled, IntelliJ IDEA will ask you whether you want to share project settings files via VCS. You can choose Always Add to synchronize project settings with other repository users who work with IntelliJ IDEA. 

If there is no Enable Version Control Integration option available in the VCS Operations Popup , it means that Git version control is already enabled for the project. 

### Associate different directories within the project with different Git repositories 

Open the directory that you want to put under Git. 

In the main menu, go to VCS | Create Git Repository . 

In the dialog that opens, specify the directory where a new Git repository will be created. 
Git does not support external paths, so if you choose a directory that is outside your project root, make sure that the folder where the repository is going to be created also contains the project root. 

If you are creating multiple Git repositories inside the project structure, repeat the previous steps for each directory. 
After you have initialized a Git repository for your project, you need to add project files to the repository. 

### Add files to the local repository 

In the Commit tool window ( Alt+0 ) , expand the Unversioned Files node. 

Select the files you want to add to Git or the entire changelist and press Ctrl+Alt+A or choose Add to VCS from the context menu. 
You can also add files to your local Git repository from the Project tool window ( Alt+1 ) : select the files you want to add, and press Ctrl+Alt+A or choose Git | Add from the context menu. 
When Git integration is enabled in your project, IntelliJ IDEA suggests adding each newly created file under Git, even if it was added from outside IntelliJ IDEA. You can change this behavior in the Version Control | Confirmation settings page Ctrl+Alt+S . If you want certain files to always remain unversioned, you can ignore them . 
If you attempt to add a file that's on the .gitignore list, IntelliJ IDEA will suggest force-adding it. Clicking Cancel in the confirmation dialog only cancels force-adding ignored files – all other files will be added to the Git repository. 

## Exclude files from version control (ignore) 

Sometimes you may need to leave certain files unversioned. These can be VCS administration files, artifacts of utilities, backup copies, and so on. You can ignore files through IntelliJ IDEA, and the IDE will not suggest adding them to Git and will highlight them as ignored. 
You can only ignore unversioned files, that is, files that you see in the Unversioned Files changelist. If a file is added to Git but not committed , you can right-click it in the Commit window and choose Rollback . 
Git lets you list ignored file patterns in two kinds of configuration files: 

.git/info/exclude file. 
Patterns listed in this file only apply to the local copy of the repository. 
This file is created automatically when you initialize or check out a Git repository. 

One or more .gitignore files in the VCS root directory and its subdirectories. 
These files are checked into the repository so that the ignore patterns in them are available to the entire team. Therefore, it is the most common place to store the ignored file patterns. 
If there is no .gitignore file in the VCS root directory, you can right-click anywhere in the Project tool window, choose New | File , and type .gitignore in the New File dialog. 
To create a .gitignore file in Windows Explorer, create a file named .gitignore. and Windows will rename it automatically to .gitignore . 

### Add files to .gitignore or .git/info/exclude 

Decide what kind of Git configuration file you are going to use to ignore files. If in doubt, use .gitignore . 

Locate the unversioned file or folder you want to ignore in the Commit window or in the Project tool window ( Alt+1 ) . File colors in these views help you identify the status of the file. 

Right-click the selection and choose Git | Add to .gitignore or Git | Add to .git/info/exclude . 
File colors in these views help you identify the status of the file. 
If you need to exclude files by a certain pattern or type, you can edit the .gitignore or .git/info/exclude file directly. See .gitignore patterns format . 
If you want ignored files to be also displayed in the Commit window, click on the toolbar and select Show Ignored Files . 

## Add a remote repository 

If you created a Git repository based on local sources, you need to add a remote repository to be able to collaborate on your Git project, as well as to eliminate the risks of storing your whole codebase locally. You push changes to a remote repository when you need to share your work and pull data from it to integrate changes made by other contributors into your local repository version. 
If you have cloned a remote Git repository , for example, from GitHub , the remote is configured automatically, and you do not have to specify it when you want to sync with it. The default name Git gives to the remote you've cloned from is origin . 
For more information about sharing project settings for different project formats, refer to Share project settings through VCS . 

### Define a remote 

Create an empty repository on any Git hosting, such as Bitbucket , GitHub , or GitLab . You can create a repository on GitHub or GitLab without leaving IntelliJ IDEA: see Share a project on GitHub and Share a project on GitLab . 

Invoke the Push dialog when you are ready to push your commits by selecting Git | Push from the main menu, or press Ctrl+Shift+K . 

If you haven't added any remotes so far, the Define remote link will appear instead of a remote name. Click it to add a remote. 

In the dialog that opens, specify the remote name and the URL where it will be hosted. 
Select the Fetch remote option to also fetch the branch list and commit history right away. 

### Add a second remote 

In some cases, you also need to add a second remote repository. This may be useful, for example, if you have cloned a repository that you do not have write access to and are going to push changes to your own fork of the original project. Another common scenario is that you have cloned your own repository, which is somebody else's project fork, and you need to synchronize with the original project and fetch changes from it. 

In the main menu, go to Git | Manage Remotes . The Git Remotes dialog will open. 

Click the Add button on the toolbar or press Alt+Insert . 

In the dialog that opens, specify the remote name and URL and click OK . 

To edit a remote (for example, to change the name of the original project that you have cloned), right-click the remote branch in the Branches pane of the Git Log tool window and select Edit Remote from the context menu. 
You can also edit a remote from the Push Dialog by clicking its name. 

To remove a repository that is no longer valid, right-click it in the Branches pane of the Git Log tool window , and select Remove Remote from the context menu. 
Learn more from this video: 

## Set passwords for Git remotes 

Every time you interact with a remote Git repository (for example, during a pull , update , or push operation), it requires authorization. You can configure IntelliJ IDEA to remember your passwords so that you do not have to specify your credentials each time authorization is required. 

### Configure a password policy 

In the Settings dialog Ctrl+Alt+S , select Appearance and Behavior | System Settings | Passwords on the left. 

Select how you want IntelliJ IDEA to process passwords for Git remote repositories: 

In native Keychain : select this option to use native Keychain to store your passwords. This setting is only available for macOS and Linux. 

In KeePass : select this option to use the KeePass password manager to store your passwords. When you use the KeePass password manager , a master password will be used to access the file that stores individual passwords. Once IntelliJ IDEA remembers your passwords, it will not ask for them unless you need to access the password database. Enter the password that will be used to access the c.kdbx file in the MasterPassword field. 
You can change the default location of the c.kdbx file in the Database field. 
To import a c.kdbx file, click and select Import from the drop-down menu or click and specify the path to a local file containing your passwords. 
If you want to remove the existing passwords from the database, select Clear . 

Do not save, forget passwords after restart : select this option if you want your passwords to be reset after you close IntelliJ IDEA. 17 August 2026 Tutorial: Getting started with Git in IntelliJ IDEA Add files to Git and track changes
