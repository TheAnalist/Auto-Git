mkdir "%APPDATA%\Auto-Git"
move "auto-git.exe" "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup"
cd "%APPDATA%\Auto-Git"
type NUL > .git-project
auto-git.exe