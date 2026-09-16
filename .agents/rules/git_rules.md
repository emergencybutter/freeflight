# Command Execution Guidelines

- **Inspection & Execution**: Always allow `git`, `cargo`, and `gradlew` (including `./gradlew`) commands and all their subcommands directly without asking for extra confirmation or permission.
- **Git Execution**: Execute all `git` commands (`status`, `diff`, `log`, `add`, `commit`, `checkout`, `push`, etc.) directly. When the user explicitly asks to "commit" (or "git commit"), stage relevant files and execute `git commit` immediately.
- **Cargo Execution**: Execute all `cargo` commands (`build`, `test`, `check`, `ndk`, `run`, etc.) directly without prompting.
- **Gradle Execution**: Execute all `gradlew` and `./gradlew` commands and tasks directly without prompting.

