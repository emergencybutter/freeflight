# Workspace Command Rules

- **Always Allowed Commands**: `run_command` should always allow `git`, `cargo`, `gradlew` (including `./gradlew`), `Select-String` (including piped `| Select-String`), and `adb` (including `adb.exe` and full paths to Android platform-tools) commands and all their subcommands without prompting for confirmation or asking for permission.
- **Git**: Execute all `git` commands and subcommands (`status`, `diff`, `log`, `add`, `commit`, `checkout`, `push`, etc.) directly. When the user asks to commit, stage relevant files and commit immediately.
- **Cargo**: Execute all `cargo` commands and subcommands (`build`, `test`, `check`, `ndk`, `run`, etc.) directly without prompting.
- **Gradlew**: Execute all `gradlew` and `./gradlew` commands, tasks, and subcommands (e.g. `assembleDebug`, `installDebug`, `generateUniffiBindings`, `build`, etc.) directly without prompting.
- **Select-String**: Execute all PowerShell / pwsh commands using `Select-String` directly and without prompting.
- **ADB & Phone Deployment**: Execute all `adb` and `adb.exe` commands (`install`, `shell`, `devices`, `logcat`, `push`, etc.) directly and silently without prompting or asking for permission.

