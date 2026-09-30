# Crafting a package: chapter briefs

## Package anatomy and submilli build

- **Purpose:** Show what a package is made of and how `submilli build` turns it into something a blueprint can grant.
- **Starting point:** The quickstart's package, the blueprint chapter's grants, and the language and standard-library chapters.
- **Understanding:** Readers can name the files of a project, explain the difference between `@capability` and `check`, say why a check's fields are a design decision, read `capabilities.yaml`, and declare a dependency.
- **Action:** Scaffold a project, write an operation that checks before it acts, build and publish it locally, and run a program under a blueprint that is allowed and one that is denied.
- **Boundaries:** Policy grammar belongs to Permissions; installing on a server to Submilli server; tests and editor files to their own chapters.
- **Evidence:** `crates/submilli/src/commands/build.rs`, `crates/submilli-build/src/{lib,scaffold,capabilities}.rs`, and a scratch project.

## Editor setup

- **Purpose:** Say what `build init` sets up for an editor, and where the editor's view stops.
- **Understanding:** Readers can say which files are theirs and which are generated, when the generated ones are refreshed, and why the compiler, not the editor, is the checker.
- **Action:** Open a project in an editor and run the VS Code build task.
- **Evidence:** `crates/submilli-build/src/scaffold.rs`; `tsc -p .` over a scratch project.

## Testing packages

- **Purpose:** Show how to test a package and what the tests do and don't prove.
- **Understanding:** Readers can explain labels and how a failure ends a file, how a live test gets its secret, which files count as HTTP tests, and why package tests don't test policy.
- **Action:** Write and run tests, including one that skips without its key.
- **Evidence:** The `test_runner` module in `crates/submilli/src/commands/build.rs`, `crates/submilli-build/src/doc_examples.rs`, and a scratch project.

## Verification

- Built `@acme/billing` (two source files) and `@acme/support` (a sibling dependency) in a scratch project with an isolated `SUBMILLI_HOME`. The build, test, and publish output in the chapters is from those runs, with paths shortened.
- Ran a failing test, a failing readme example, an undeclared dependency, a compile error, and a host passed in a parameter, for the errors quoted.
- Ran `credit.ts` under a blueprint before and after granting `acme.com/credits.apply`.
- Declared `@submilli/jina` from the local store as a dependency and built. The GitHub dependency form and `submilli.lock` are from the source and were not run.
- Ran `tsc -p .` over the project with `@submilli/jina` as a store dependency: no errors, after the fixes that added `submilli:security` and dependency declarations to the generated types.
- The package-side `@capability`/`check` warnings and the rule that `check` sits directly in an exported function come from the check-discipline change in progress in `submilli-wt1` (SUB-1117). The chapter describes that behavior; on `main` before it lands, package builds don't report them.
- The VS Code build task was read, not run in VS Code.
- The testing chapter's coding-agent section describes a Claude Code run with the skill installed.
- The package-anatomy coding-agent section is a Claude Code run with the skill against a live Attio workspace, key passed as `ATTIO_API_KEY` from the repo's `.env` without being printed. Read-only prompt; about twelve minutes, 32 turns. An earlier read-write run with the same workspace created nothing in it and tested writes only with unit tests, which led to the "Packages that write" advice. Company names from the workspace are left out of the chapter.
