# agent-receipts

Check a coding report against a local Git repository and see the evidence for each verdict.
This walking skeleton checks commit existence, file presence at HEAD, and whether the working tree is clean.
It uses fixed statement rules and runs offline.

## Run a check

Install stable Rust and Git, then build the executable.

```sh
CARGO_BUILD_JOBS=2 cargo build --locked --release
./target/release/agent-receipts --repo . --report examples/synthetic-report.md
printf 'Tree is clean\n' | ./target/release/agent-receipts --repo .
./target/release/agent-receipts --help
```

`--report -` reads stdin and is the default.
The sample report is synthetic and intentionally includes missing evidence and false statements.
Results depend on the checked repository, so an uncommitted checkout cannot confirm file presence at HEAD.

Use one statement per line, optionally with a Markdown bullet and a final period.

```text
Committed as <real commit hash>
File src/main.rs exists
Added `path with spaces.txt`
Tree is clean
```

The output is a tab-separated table with source line numbers, the check performed, a verdict, and evidence.
Exit codes are 0 for all confirmed, 1 for any contradiction, 2 for input or repository errors, and 3 for missing evidence.
An empty report exits 3.

## Run the synthetic demo

After building, run the demo against its own temporary repository.
It checks successful claims, introduces an untracked file, and shows a missing file and an unsupported test claim.
The script checks its expected exit codes and removes its temporary repository when it finishes.

```sh
sh examples/demo.sh
```

## Limits

`Added path` checks only whether a file blob exists at HEAD.
It does not prove a new addition, authorship, or a change since a base commit.
A clean-tree check includes staged, unstaged, and untracked changes, and excludes ignored files.
Quoted code and unsupported wording receive `not checkable` verdicts.
The tool does not execute report commands or check test counts, captured logs, remote refs, or push history in this phase.
It cannot prove when tests ran or whether supplied evidence is authentic.

See [SPEC.md](SPEC.md) for the exact grammar, evidence limits, planned log and remote checks, test plan, and two-minute demo script.

## Check the source

```sh
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --locked --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked --all-targets
CARGO_BUILD_JOBS=2 cargo build --locked --release
```

Tests create isolated repositories under the system temp directory.
The [MIT license](LICENSE) applies.
