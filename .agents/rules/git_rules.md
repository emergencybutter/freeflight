# Git Execution Guidelines

- **Inspection**: Always allow `git diff`, `git status`, and standard git read-only inspection subcommands without asking for extra confirmation.
- **Commit Execution**: When the user explicitly asks to "commit" (or "git commit"), stage relevant modified and untracked files (`git add`) and execute `git commit` directly using terminal tools without prompting the user for permission or confirmation.
