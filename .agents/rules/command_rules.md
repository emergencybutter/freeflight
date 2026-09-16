# Command Execution Guidelines

- **Always Allowed Commands**: `run_command` should always allow `git`, `cargo`, `gradlew` (including `./gradlew`), and `Select-String` (including piped `| Select-String`) commands and all their subcommands without prompting for confirmation or asking for permission.
- **Git**: Execute all `git` commands and subcommands (`status`, `diff`, `log`, `add`, `commit`, `branch`, `checkout`, `push`, etc.) directly.
- **Cargo**: Execute all `cargo` commands and subcommands (`build`, `test`, `check`, `ndk`, `run`, etc.) directly.
- **Gradlew**: Execute all `gradlew` and `./gradlew` commands, tasks, and subcommands (e.g., `assembleDebug`, `generateUniffiBindings`, `build`, etc.) directly.
- **Select-String**: Execute all PowerShell / pwsh commands using `Select-String` directly without prompting.
