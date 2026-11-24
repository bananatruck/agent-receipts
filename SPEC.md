# Specification

## Purpose

`agent-receipts` checks explicit statements in a coding report against local Git evidence and shows a verdict with its evidence.
The user is a developer or reviewer who needs to check a final report before accepting work.
A plausible report can name missing files, nonexistent commits, or a clean tree that still has changes.
The tool checks observable facts and preserves uncertainty when evidence is missing.

## First statement types

| Statement | Evidence | This phase |
| --- | --- | --- |
| `Committed as <hash>` or `Commit <hash> exists` | A unique local object whose type is `commit` | Implemented |
| `File <path> exists` or `Added <path>` | A blob at `HEAD:<path>` | Implemented |
| `Tree is clean` or `The tree is clean` | Git porcelain status, including untracked files | Implemented |
| `N tests passed` | An explicitly supplied command log with a matching command and summary | Specified only |
| `Not pushed` | The current commit and evidence for a named remote ref | Specified only |

Commit hashes contain 7 to 64 hexadecimal characters and may be abbreviated if they identify exactly one local object.
A missing local object contradicts local existence, even if another repository might have the object.
Ambiguous abbreviations are not checkable.
A file verdict concerns presence at HEAD, even when the report uses `Added`.
It does not establish that the file was newly added, changed since a base, or written by a particular person.
The table names this reduced check as `file at HEAD` rather than confirming authorship or an addition.
A directory or submodule is not a file blob.
A symbolic link counts as its stored blob without following the link.
File paths are relative to the repository root and cannot contain parent components, absolute roots, or Git revision syntax.
Missing HEAD makes file statements not checkable.
Clean means no staged, unstaged, or untracked changes in the working tree, including submodule changes reported by Git.
Ignored files do not make the tree dirty.
A clean-tree verdict describes the observation time and cannot prove what the tree looked like earlier.

## Report grammar

Reports are UTF-8 plain text or Markdown from stdin or a file.
Each nonempty line contains one statement.
The parser accepts an optional Markdown bullet or numbered-list prefix and an optional final period.
Statement keywords are case insensitive.
Hashes and paths may have a single pair of Markdown backticks.
Paths with spaces require backticks.
Blank lines and Markdown ATX headings are presentation and produce no verdict.
Fenced code and lines indented by four spaces or a tab are quoted material and cannot confirm a statement.
Fence delimiter lines produce no verdict.
Every other nonempty line produces a verdict, including unsupported wording, negations, conditional statements, and quoted examples.
Unsupported lines are not checkable and retain their source line number.
The parser does not infer claims from arbitrary hash-shaped words or backticked commands.
Statements embedded in a longer sentence are not checkable.
No report text is executed.

## Command line

```text
agent-receipts [--repo PATH] [--report PATH|-]
agent-receipts --help
agent-receipts --version
```

`--repo` defaults to the current directory and can name a subdirectory of a working repository.
`--report` defaults to `-`, which reads stdin until EOF.
An interactive terminal without a report fails with an actionable error instead of waiting for input.
Report file paths resolve from the invocation directory, independently of `--repo`.
The executable writes a tab-separated verdict table to stdout and operational errors to stderr.
Columns are `LINE`, `STATEMENT`, `CHECK`, `VERDICT`, and `EVIDENCE`.
Cell escaping renders tabs, carriage returns, backslashes, and control characters visibly so report text cannot inject rows or terminal escapes.
Verdicts are `confirmed`, `contradicted`, or `not checkable`.
The final summary gives the number of each verdict.
An empty report has no affirmative evidence and exits with code 3.
The tool never writes to the checked repository or contacts a remote.
Git subprocesses use explicit arguments without a shell, disable filesystem monitor hooks, and suppress optional index locks.
Repository-location environment variables are removed so `--repo` selects the repository being checked.

| Exit | Meaning |
| --- | --- |
| 0 | At least one statement, all confirmed |
| 1 | At least one contradiction, including mixed reports |
| 2 | Invalid invocation, unreadable input, unavailable Git, or invalid working repository |
| 3 | No contradictions, but at least one uncheckable statement or no statements |

Contradictions take precedence over uncheckable statements.
A per-statement Git failure produces not checkable, never a fabricated contradiction.
Help and version requests exit 0 without reading input or opening a repository.

## Later evidence adapters

The proposed `--log COMMAND=PATH` option associates a command with a caller-supplied captured log.
A test-count check requires a matching command record and an unambiguous completed summary for that command.
Missing logs, truncated output, multiple possible runs, or unsupported summary formats produce not checkable.
A summary count different from the report produces contradicted.
A matching summary confirms only what the supplied log records, not that an untampered command ran in this repository.
The proposed `--remote-ref REMOTE:REF` option identifies the exact remote branch for a pushed-state check.
A cached tracking ref cannot establish the current remote state and is not sufficient to confirm `not pushed`.
A later explicit online mode could obtain a timestamped remote snapshot and compare ancestry against it.
Such a verdict would be limited to that named ref and observation time, and could not establish that no push ever happened.
Missing remote evidence produces not checkable.
These options are not accepted by the walking skeleton.

## Implementation and exclusions

This is new work started on November 24, 2026.
Git runs as a subprocess because its object lookup and status behavior already define the facts being checked.
The executable uses `clap` for arguments and `regex` for the fixed statement grammar.
Integration tests use `tempfile` for isolated repositories under the system temp directory.
There are no model calls, network requests, credentials, or commands copied from report text.
Arbitrary natural-language interpretation, command execution, test-log verification, remote verification, and diff-since-base checks are outside this phase.
The tool does not prove test execution, intent, authorship, code quality, or report completeness.
It assumes the local Git executable and object store are trustworthy.
Repository evidence is not an atomic snapshot if another process changes the repository during verification.

## Test plan

The public seam is the executable with stdin or a report file, a real repository, stdout, stderr, and an exit code.
Every repository integration test creates its own throwaway Git repository under the system temp directory.
Tests set local identity and disable signing and hooks without using personal Git configuration.
Each behavior starts with a failing integration test before its implementation.
The first slice checks present and missing commits and files in one report.
The next slice checks clean and dirty trees with staged, unstaged, and untracked changes.
Further cases cover unsupported claims, empty reports, Markdown, quoted code, invalid paths, unborn HEAD, directories, non-commit objects, and input errors.
Tests check that workspace-only files do not confirm HEAD presence and that working-copy deletions do not erase HEAD evidence.
The CI checks are formatting, clippy with warnings denied, integration tests, and a release build.

## Two-minute demo script

All report examples in `examples/` are synthetic.
The recording uses a fresh temporary repository so the checked repository stays untouched.

| Time | Action |
| --- | --- |
| 0:00 to 0:20 | Explain that a report is a set of statements and each verdict includes the evidence it checks. |
| 0:20 to 0:40 | Create a throwaway repository, add `src/lib.rs`, commit, and capture its real hash. |
| 0:40 to 1:00 | Pipe a synthetic report with that hash, `File src/lib.rs exists`, and `Tree is clean` into the executable. |
| 1:00 to 1:20 | Add an untracked file and rerun to show the contradicted clean-tree statement and exit 1. |
| 1:20 to 1:40 | Add `File src/missing.rs exists` and `77 tests passed` to show contradicted and not checkable verdicts together. |
| 1:40 to 2:00 | Show the source line numbers, the limited meaning of file presence, and the planned log and remote checks. |

The [GenHacks rules](https://genhacks-open.devpost.com/rules) require new work after November 1, 2026 and disclosure of libraries and AI tools in the project description.
The submission needs a code link and an embedded demo video of at most three minutes.
The chosen track is `AI, Agentic Systems, Cybersecurity, and Dev Tools`.
Registration, eligibility confirmation, publication, the required disclosure, and submission remain outside this phase.
