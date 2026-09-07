#!/bin/sh

# Apply the formatters, or check that the tree is already where they would put
# it.
#
# One file list and one order, used by "make indent" through --write and by
# "make check" through --check, so what gets rewritten is what gets checked.
#
# The order is not a preference. commentflow puts a blank line before a comment
# that sits inside a method chain, an array literal or an argument list, and
# cargo fmt takes that line back out; running the formatters last lets them have
# the final word. That is also why the check runs the whole chain against a copy
# of the tree rather than asking commentflow alone whether it would change
# something: on its own it always would, and the answer would be a gate nobody
# can satisfy.
#
# Style flags are deliberately absent. shfmt reads .editorconfig, which is where
# this repository's shell settings already live, and passing -i here would
# silently override the file for one caller.

set -u

mode=check
case "${1:-}" in
    "" | --check) ;;
    --write) mode=apply ;;
    *)
        echo "usage: ${0##*/} [--check | --write]" >&2
        exit 2
        ;;
esac

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT" || exit 1

# Untracked but non-ignored files are included, so a new script is covered
# before it is staged. Ignored ones are not, which keeps the generated
# src/engine/s2t_data.rs out of every list here and therefore out of the drift
# comparison below. It is not out of cargo fmt: that walks the crate from its
# root and finds the file whatever this list says, which is why the copy the
# check runs against carries a stub instead. A move that is not staged yet
# leaves the old path in the index and not in the worktree, and tar below would
# stop the whole run on it, so drop exactly the paths git reports as deleted.
# Not everything that fails a test for existence: a name holding a newline
# arrives here quoted and so looks absent, and it has to reach the unusable
# guard below and fail loudly rather than be skipped. Skip-worktree entries,
# which is how a sparse checkout holds a path it has not materialized, are
# absent from disk and are not reported as deleted, so they have to be named
# here too or tar stops on them.
gone=$(mktemp) || exit 1
trap 'rm -f "$gone"' EXIT
trap 'rm -f "$gone"; exit 130' HUP INT TERM

# Each git run is checked on its own: a failure inside a pipeline is masked by
# the exit status of the last stage, and an empty list here would quietly hand
# every formatter a path that is not there.
deleted=$(git ls-files --deleted) || exit 1
flags=$(git ls-files -v) || exit 1

# Lowercase marks assume-unchanged, so a path carrying both bits reads 's' and
# is missed by 'S' alone while git also stops reporting it as deleted. Absence
# is what disqualifies a path, and git naming it is what makes the absence
# explainable. A skip-worktree file that is materialized still gets formatted,
# and a broken symlink or a quoted name is in neither list, so it stays and
# reaches the guard below instead of being skipped.
printf '%s\n%s\n' "$deleted" "$(printf '%s\n' "$flags" | sed -n 's/^[Ssh] //p')" \
    | grep -v '^$' | sort -u \
    | while IFS= read -r path; do

        # -L as well as -e: a dangling symlink is present in the worktree even
        # though -e follows the link and says otherwise, so it is a path this
        # run has to account for rather than drop.
        [ -e "$path" ] || [ -L "$path" ] || printf '%s\n' "$path"
    done > "$gone" || exit 1

list()
{
    git ls-files --cached --others --exclude-standard -- "$@" | sort -u \
        | grep -vxFf "$gone"
}

rust=$(list 'build.rs' 'build/*.rs' 'src/*.rs' 'tests/*.rs' 'benches/*.rs')
shell=$(list '*.sh')
python=$(list 'scripts/*.py')

# assets/ruleset.json is formatted too, by the same script that lints it. There
# is no --check for that normalization, and there does not need to be: the check
# below runs the writer against a copy and diffs, which is how every other lane
# here already works.
ruleset=$(list 'assets/ruleset.json')

# Word splitting on those lists is what makes the lanes below work, so a path
# the splitting cannot carry has to stop this script rather than be dropped from
# it. A name holding a blank becomes two paths, one starting with a dash reaches
# a formatter as an option, and git ls-files renders a name holding a newline in
# quotes, which is the tell for that case. There are none in this tree, and a
# commit that adds one should hear why it was not formatted.
unusable=$(printf '%s\n%s\n%s\n%s\n' "$rust" "$shell" "$python" "$ruleset" \
    | grep -E '^-|[[:blank:]]|^"')
if [ -n "$unusable" ]; then
    printf '%s\n' "$unusable" | sed 's/^/  /' >&2
    echo "${0##*/}: the paths above cannot be passed through a shell list" >&2
    exit 2
fi

# A skip is right on a laptop and wrong on the runner that is supposed to be
# authoritative, where a lane that stopped running is a gate that stopped
# meaning anything. ZHTW_REQUIRE_TOOLS turns every skip into a failure; CI sets
# it on the one leg that installs the tools.
skipped=
have()
{
    command -v "$1" > /dev/null 2>&1 && return 0
    if [ -n "${ZHTW_REQUIRE_TOOLS:-}" ]; then
        echo "${0##*/}: $1 is required here and is not installed" >&2
        exit 2
    fi

    # Said once rather than once per pass. format() runs the chain more than
    # once, and a tool missing on the first pass is missing on every later one.
    case " $skipped " in
        *" $1 "*) ;;
        *)
            echo "${0##*/}: $1 not installed, skipping that lane" >&2
            skipped="$skipped $1"
            ;;
    esac
    return 1
}

# Run in $1, on the files named after it. Every lane is optional in the same way
# the rest of the gate treats an uninstalled tool: a contributor without it
# should still be able to run the half that works, and CI installs the ones that
# must not skip.
format_once()
{
    if [ -n "$rust$shell" ] && have commentflow; then
        # shellcheck disable=SC2086
        commentflow $rust $shell > /dev/null || return 1
    fi
    if [ -n "$rust" ] && have cargo; then
        cargo fmt || return 1
    fi
    if [ -n "$python" ] && have black; then
        # shellcheck disable=SC2086
        black --quiet $python || return 1
    fi
    if [ -n "$shell" ] && have shfmt; then
        # shellcheck disable=SC2086
        shfmt -w $shell || return 1
    fi
}

# Outside format_once, which runs more than once: nothing in the reflow chain
# reads or writes JSON, so the normalization has nothing to converge with and
# would only say what it did once per pass.
normalize_ruleset()
{
    if [ -n "$ruleset" ] && have python3; then
        python3 scripts/check-ruleset.py || return 1
    fi
}

# One pass is not a fixpoint. commentflow wraps a comment against the
# indentation it finds, and cargo fmt and shfmt then reindent the block around
# it, so a file that needed reindenting comes out wrapped for the width it used
# to have. Running the chain once would leave "make indent" writing a tree that
# "make check" reports as drift and that the pre-commit commentflow lane
# rejects, which is a gate nobody can satisfy in one go.
#
# The bound is what tells a slow convergence from a pair of formatters that
# disagree forever. A comment that has to be rewrapped after a reindent costs
# two changing passes and a third to prove it settled, which is the worst case
# seen; the rest is headroom, and it is free because the loop leaves as soon as
# a pass changes nothing.
format()
{
    # Seeded before the first pass, not after it: a tree the formatters would
    # not touch is the common case, and it should cost one pass rather than the
    # two an empty seed forces.
    # shellcheck disable=SC2086
    previous=$(cat $rust $shell $python 2> /dev/null | cksum)
    attempt=0
    while [ "$attempt" -lt 5 ]; do
        format_once || return 1

        # The whole list as one stream: a pass that moved a byte anywhere moves
        # this, and nothing here cares which file it was.
        # shellcheck disable=SC2086
        current=$(cat $rust $shell $python 2> /dev/null | cksum)
        if [ "$current" = "$previous" ]; then
            normalize_ruleset
            return
        fi
        previous=$current
        attempt=$((attempt + 1))
    done

    echo "${0##*/}: the formatters did not agree after 5 passes" >&2
    return 1
}

if [ "$mode" = apply ]; then
    format
    exit
fi

# The check writes too, just not here: the chain runs against a copy so that a
# dirty tree is never rewritten by a gate. Five files come along that no lane
# formats, because a formatter that cannot find its configuration in the copy
# would judge the copy by different rules than the ones that wrote the tree:
# Cargo.toml, which cargo fmt reads to find the crate; rust-toolchain.toml,
# without which rustup hands the copy whatever toolchain is default and the two
# runs disagree about which rustfmt they mean; .editorconfig, which shfmt reads
# for the style; .clang-format, which commentflow reads for the column limit;
# and scripts/schema-facts.json, without which check-ruleset.py refuses to run
# at all.
work=$(mktemp -d) || exit 2

# The signal traps exit rather than falling through: a handler that only cleans
# up leaves the rest of the script running against a directory it just deleted,
# which reports every file as drift. A shell keeps one handler per signal, so
# these replace the pair set for $gone above and have to remove it too or the
# check mode leaks one file per run.
trap 'rm -rf "$work" "$work.tar"; rm -f "$gone"' EXIT
trap 'rm -rf "$work" "$work.tar"; rm -f "$gone"; exit 130' HUP INT TERM

files=$(printf '%s\n%s\n%s\n%s\n' "$rust" "$shell" "$python" "$ruleset" | grep -v '^$')

# One tar rather than a mkdir and a cp per file: the file list here is in the
# hundreds, and a process pair for each of them was most of this gate's wall
# clock in the tree this was adapted from. Through an archive on disk rather
# than a pipe, because a shell pipeline reports only its last command and a
# failed pack behind a successful unpack is an empty copy that every later step
# then reads as formatting drift.
printf '%s\n%s\n%s\n%s\n%s\n%s\n' "$files" Cargo.toml rust-toolchain.toml \
    .editorconfig .clang-format scripts/schema-facts.json \
    | tar -cf "$work.tar" -T - || exit 2
(cd "$work" && tar -xf "$work.tar") || exit 2
rm -f "$work.tar"

# cargo fmt resolves every "mod" declaration from the crate root, and
# src/engine/s2t_data.rs is generated and gitignored, so it is not in the copy.
# A stub resolves the module, and it holds a newline rather than nothing because
# rustfmt reports a diff against a zero-byte file. Nothing here compares that
# file, and carrying 43k generated lines into the copy to reformat them would be
# the slowest lane in the gate by a wide margin.
mkdir -p "$work/src/engine" || exit 2
[ -e "$work/src/engine/s2t_data.rs" ] \
    || printf '\n' > "$work/src/engine/s2t_data.rs" || exit 2

# Not a subshell: have() records the lanes it skipped, and a variable set inside
# a subshell does not come back to the verdict that has to report them.
cd "$work" || exit 2
format || exit 2
cd "$ROOT" || exit 2

drift=
for file in $files; do
    cmp -s "$file" "$work/$file" || drift="$drift $file"
done

if [ -n "$drift" ]; then
    echo "Formatting drift; run 'make indent':" >&2
    for file in $drift; do
        echo "  $file" >&2
    done
    exit 1
fi

# A verdict that does not say what it skipped reads as a verdict on everything.
# The lane messages above go to standard error, which is not where anybody looks
# in a green log.
if [ -n "$skipped" ]; then
    echo "Formatting clean, except these lanes had no tool:$skipped"
else
    echo "Formatting clean."
fi
