#!/usr/bin/env bash
# Mechanical docs checks. Both codebases this project learned from had prose that lied about the
# code; these checks are how mav keeps its own docs honest. Run from anywhere; CI runs it on every
# push.
set -u
cd "$(dirname "$0")/.." || exit 1

failures=0
fail() {
    echo "check_docs: $1" >&2
    failures=$((failures + 1))
}

# CLAUDE.md and AGENTS.md are the same map for different harnesses; they must be byte-identical.
if ! cmp -s CLAUDE.md AGENTS.md; then
    fail "CLAUDE.md and AGENTS.md differ (they must be byte-identical)"
fi

# Every relative markdown link must resolve to a real file.
md_files=$(find . -name '*.md' -not -path './.git/*' -not -path './core/target/*')
for file in $md_files; do
    dir=$(dirname "$file")
    links=$(grep -oE '\]\([^)#]+\)' "$file" 2>/dev/null | sed -E 's/^\]\(//; s/\)$//')
    for link in $links; do
        case "$link" in
        http://* | https://* | mailto:*) continue ;;
        esac
        target="$dir/$link"
        if [ ! -e "$target" ]; then
            fail "$file links to $link, which does not exist"
        fi
    done
done

# Every skill folder needs a SKILL.md whose frontmatter name matches the folder.
for skill_dir in skills/*/; do
    name=$(basename "$skill_dir")
    skill_file="$skill_dir/SKILL.md"
    if [ ! -f "$skill_file" ]; then
        fail "$skill_dir has no SKILL.md"
        continue
    fi
    declared=$(awk -F': *' '/^name:/ {print $2; exit}' "$skill_file")
    if [ "$declared" != "$name" ]; then
        fail "$skill_file declares name '$declared' but lives in '$name/'"
    fi
done

# The plan lifecycle directories must exist so packets always have somewhere to move.
for dir in docs/plans/active docs/plans/completed docs/adr; do
    if [ ! -d "$dir" ]; then
        fail "expected directory $dir is missing"
    fi
done

# Every milestone plan file must be indexed in docs/plans/README.md.
for plan in docs/plans/active/*.md docs/plans/completed/*.md; do
    [ -e "$plan" ] || continue
    rel=${plan#docs/plans/}
    if ! grep -q "$rel" docs/plans/README.md; then
        fail "$plan is not listed in docs/plans/README.md"
    fi
done

if [ "$failures" -gt 0 ]; then
    echo "check_docs: $failures problem(s)" >&2
    exit 1
fi
tools/check_no_bundled_connectors.py || exit 1
python3 tools/check_model_assets.py || exit 1
# The prose in docs/ml.md against the artefacts it describes. The generated tables have their
# own generator and their own --check; this is for the sentences around them, which is where
# "all but two" survived a change that made it one.
python3 tools/ml/check_claims.py || exit 1
# The capability matrix against the pipeline table and the manifest it describes. Generated, so
# a re-conversion or a newly ported front-end has to regenerate it rather than leave it lying.
# Stock python3: the generator reads committed JSON and Rust, and the conversion virtualenv it
# was reaching for is local-only and gitignored, so a gate that needed it would never run in CI.
python3 tools/ml/model_matrix.py --check || exit 1
# Every committed file whose header says "Do not edit" still matches what generates it. Runs here
# rather than only in CI because a stale generated file is the kind of thing you want to hear
# about before the commit, not after.
python3 tools/check_generated.py || exit 1
echo "check_docs: ok"
