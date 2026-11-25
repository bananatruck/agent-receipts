#!/bin/sh
set -eu

project_dir=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary="$project_dir/target/release/agent-receipts"
if [ ! -x "$binary" ]; then
    echo 'Build first with: CARGO_BUILD_JOBS=2 cargo build --locked --release' >&2
    exit 2
fi

fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR
unset GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NAMESPACE
unset GIT_CONFIG_PARAMETERS GIT_REPLACE_REF_BASE GIT_CONFIG
export GIT_CONFIG_COUNT=0
git init --quiet --initial-branch=main "$fixture"
git -C "$fixture" config user.name Demo
git -C "$fixture" config user.email demo@example.invalid
git -C "$fixture" config commit.gpgsign false
git -C "$fixture" config core.hooksPath /dev/null
mkdir "$fixture/src"
printf 'pub fn value() -> u8 { 7 }\n' > "$fixture/src/lib.rs"
git -C "$fixture" add src/lib.rs
git -C "$fixture" commit --quiet -m 'Add fixture'
commit=$(git -C "$fixture" rev-parse HEAD)

check_report() {
    expected=$1
    set +e
    printf '%s\n' "$report" | "$binary" --repo "$fixture"
    result=$?
    set -e
    if [ "$result" -ne "$expected" ]; then
        echo "Expected exit $expected, got $result" >&2
        exit 1
    fi
    printf 'Exit: %s\n\n' "$result"
}

report=$(printf 'Committed as %s\nFile src/lib.rs exists\nTree is clean\n' "$commit")
echo 'Synthetic demo: matching local evidence'
check_report 0
printf 'untracked\n' > "$fixture/notes.txt"
echo 'Synthetic demo: a changed working tree'
check_report 1
report=$(printf '%s\nFile src/missing.rs exists\n77 tests passed\n' "$report")
echo 'Synthetic demo: a missing file and an unsupported test claim'
check_report 1
report='# Synthetic report
77 tests passed
Not pushed'
echo 'Synthetic demo: missing evidence does not pass'
check_report 3
