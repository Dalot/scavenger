#!/usr/bin/env bash
# release.sh — Bump version, update CHANGELOG from merged PRs, tag, and create GitHub release.
#
# Usage: scripts/release.sh 0.2.3
#
# Requirements:
#   - gh CLI authenticated
#   - Clean working tree
#   - On master branch
#
# What it does:
#   1. Validates semver argument
#   2. Runs cargo test + cargo clippy (aborts on failure)
#   3. Fetches merged PRs since last git tag
#   4. Generates CHANGELOG entries from PR titles
#   5. Bumps Cargo.toml version
#   6. Commits, tags, pushes
#   7. Creates GitHub release

set -euo pipefail

# --- Helpers ---

die() { echo "error: $*" >&2; exit 1; }

validate_semver() {
    if ! [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        die "invalid version '$1' — expected semver like 0.2.3"
    fi
}

categorize_pr() {
    local title="$1"
    case "$title" in
        feat:*|feat\(*)    echo "Added" ;;
        fix:*|fix\(*)      echo "Fixed" ;;
        refactor:*|refactor\(*)  echo "Changed" ;;
        docs:*|docs\(*)    echo "Changed" ;;
        chore:*|chore\(*)  echo "Changed" ;;
        deps:*|deps\(*)    echo "Changed" ;;
        perf:*|perf\(*)    echo "Changed" ;;
        ci:*|ci\(*)        echo "" ;;  # skip CI-only PRs
        test:*|test\(*)    echo "" ;;  # skip test-only PRs
        *)                 echo "Changed" ;;
    esac
}

# --- Main ---

VERSION="${1:-}"
[[ -z "$VERSION" ]] && die "usage: $0 <version>"
validate_semver "$VERSION"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

# Check we're on master
BRANCH="$(git branch --show-current)"
[[ "$BRANCH" != "master" ]] && die "must be on master branch (currently on '$BRANCH')"

# Check clean working tree
if ! git diff --quiet || ! git diff --cached --quiet; then
    die "working tree is not clean — commit or stash changes first"
fi

echo ""
echo "=== Scavenger Release v${VERSION} ==="
echo ""

# Step 1: Run tests
echo "[1/7] Running cargo test..."
cargo test --quiet 2>&1 || die "tests failed — aborting release"
echo "  tests passed"

# Step 2: Run clippy
echo "[2/7] Running cargo clippy..."
cargo clippy --quiet 2>&1 || die "clippy found warnings/errors — aborting release"
echo "  clippy clean"

# Step 3: Find last tag
LAST_TAG="$(git describe --tags --abbrev=0 2>/dev/null || echo "")"
[[ -z "$LAST_TAG" ]] && die "no previous git tag found"
echo "[3/7] Last tag: $LAST_TAG"

# Step 4: Fetch merged PRs since last tag
echo "[4/7] Fetching merged PRs since $LAST_TAG..."

TAG_DATE="$(git log -1 --format='%ci' "$LAST_TAG" | cut -d' ' -f1)"
PR_JSON="$(gh pr list --state merged --base master --limit 100 --json title,number,mergedAt --jq ".[] | select(.mergedAt > \"$TAG_DATE\") | {title, number}")"

if [[ -z "$PR_JSON" ]]; then
    die "no merged PRs found since $LAST_TAG"
fi

# Step 5: Generate CHANGELOG section
echo "[5/7] Generating CHANGELOG..."

TODAY="$(date +%Y-%m-%d)"

# Collect entries by category
declare -a ADDED=() FIXED=() CHANGED=()

while IFS= read -r line; do
    title="$(echo "$line" | sed 's/^title: //')"
    # Strip PR number prefix if present (e.g., "#17 fix: ...")
    title="$(echo "$title" | sed 's/^#[0-9]* //')"
    category="$(categorize_pr "$title")"
    [[ -z "$category" ]] && continue

    case "$category" in
        Added)   ADDED+=("- $title") ;;
        Fixed)   FIXED+=("- $title") ;;
        Changed) CHANGED+=("- $title") ;;
    esac
done <<< "$PR_JSON"

# Build the new section
SECTION="## [${VERSION}] - ${TODAY}"
[[ ${#ADDED[@]} -gt 0 ]] && SECTION="${SECTION}"$'\n\n'"### Added"$'\n'"$(printf '%s\n' "${ADDED[@]}")"
[[ ${#FIXED[@]} -gt 0 ]] && SECTION="${SECTION}"$'\n\n'"### Fixed"$'\n'"$(printf '%s\n' "${FIXED[@]}")"
[[ ${#CHANGED[@]} -gt 0 ]] && SECTION="${SECTION}"$'\n\n'"### Changed"$'\n'"$(printf '%s\n' "${CHANGED[@]}")"

echo "  Generated section:"
echo "$SECTION" | sed 's/^/    /'

# Step 6: Update CHANGELOG.md
echo "[6/7] Updating CHANGELOG.md and Cargo.toml..."

# Replace [Unreleased] with the new section, then add fresh [Unreleased] above
CHANGELOG="CHANGELOG.md"
TMPFILE="$(mktemp)"

# Read everything before [Unreleased]
awk '/^## \[Unreleased\]/{found=1; next} found{found=0} !found{print}' "$CHANGELOG" > "$TMPFILE"

# Insert new section + fresh Unreleased
{
    echo "## [Unreleased]"
    echo ""
    echo "$SECTION"
    echo ""
    cat "$TMPFILE"
} > "${CHANGELOG}.new"

# Update compare links at the bottom
sed -i '' "s|\[Unreleased\]: https://github.com/Dalot/scavenger/compare/v[0-9.]*...HEAD|[Unreleased]: https://github.com/Dalot/scavenger/compare/v${VERSION}...HEAD|" "${CHANGELOG}.new"
sed -i '' "s|\[HEAD\]: https://github.com/Dalot/scavenger/compare/v[0-9.]*...HEAD|[HEAD]: https://github.com/Dalot/scavenger/compare/v${VERSION}...HEAD|" "${CHANGELOG}.new"

# Add new version compare link before the old one
if grep -q "\[${LAST_TAG#v}\]:" "${CHANGELOG}.new"; then
    sed -i '' "s|\[${LAST_TAG#v}\]:|[${VERSION}]: https://github.com/Dalot/scavenger/compare/${LAST_TAG}...v${VERSION}\n[${LAST_TAG#v}]:|" "${CHANGELOG}.new"
else
    # If no version link exists, add it at the end before the last line
    LAST_LINK_LINE="$(grep -n '^\[' "${CHANGELOG}.new" | tail -1 | cut -d: -f1)"
    sed -i '' "${LAST_LINK_LINE}i\\
[${VERSION}]: https://github.com/Dalot/scavenger/compare/${LAST_TAG}...v${VERSION}
" "${CHANGELOG}.new"
fi

mv "${CHANGELOG}.new" "$CHANGELOG"
rm -f "$TMPFILE"

# Bump Cargo.toml version
sed -i '' "s/^version = \"[0-9.]*\"/version = \"${VERSION}\"/" Cargo.toml

echo "  CHANGELOG.md updated"
echo "  Cargo.toml bumped to ${VERSION}"

# Step 7: Commit, tag, push, release
echo "[7/7] Committing, tagging, and creating release..."

git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "chore: release v${VERSION}"
git tag "v${VERSION}"
git push origin master --tags

# Create GitHub release
RELEASE_BODY="$(awk "/^## \[${VERSION}\]/{found=1; next} /^## \[/{found=0} found" CHANGELOG.md)"
echo "$RELEASE_BODY" | gh release create "v${VERSION}" -F - --title "v${VERSION}"

echo ""
echo "=== Release v${VERSION} complete ==="
echo "https://github.com/Dalot/scavenger/releases/tag/v${VERSION}"
