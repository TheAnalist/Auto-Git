# Auto-Git (Windows)
Auto-Git is a lightweight Windows desktop application designed to streamline your workflow by automating the tedious parts of version control. It monitors your remote repository and pulls changes automatically, letting you focus on coding while it handles the syncing.

## Key Features
Auto-Sync: Automatically detects and downloads changes from the remote repository.

One-Click Operations: Simplified interface for the most common Git commands.

Built-in Terminal: Real-time output log so you can see exactly what Git is doing under the hood.

Safety First: Easily reset the app state if things get messy.

## The Interface

Main Controls
Push: Stage all changes and upload them to the remote repository.

Ignore: Quickly discard local modifications to keep your workspace clean and aligned with the remote.

Status: Instantly check the current state of your branch and staged files.

Menu Options
Located in the top-left corner, the menu allows you to:

Select Project: Choose your project directory (Note: The folder must contain a .git directory).

Advanced Options: (Coming Soon) Custom configurations and tweakable settings.

Reset State: Clear internal errors and refresh the application logic.

## Installation
Requirement: Auto-Git is designed exclusively for Windows.

Download the latest .exe from the Releases page.

Ensure you have Git for Windows installed.

Run Auto-Git.exe and select your project folder.

## How to Use

Open your project: Use the top-left menu to point the app to your local repository.

Watch the Terminal: The bottom panel will display live updates of fetching and pulling.

Handle Conflicts: Use the Ignore button if you want to scrap local changes and sync with the cloud, or Push to share your work.

## Troubleshooting

If the app behaves unexpectedly:

Use the Reset State option in the menu.

Check the terminal output at the bottom for specific Git error codes.

Ensure no other process is locking the .git folder.

## Roadmap

- [ ] Implement Advanced Options menu.

- [ ] Add support for multiple remote branches.

- [ ] Custom commit messages for the Push button.
