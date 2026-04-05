#!/usr/bin/env bash
set -euo pipefail

# clipboard2path-wsl release script
# Usage: ./scripts/release.sh [major|minor|patch]
# Default: auto-detect from conventional commits

REPO="ba0918/clipboard2path-wsl"
CARGO_TOML="Cargo.toml"

# ── Colors ──────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[info]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC}  $*"; }
die()   { echo -e "${RED}[error]${NC} $*" >&2; exit 1; }

# ── Pre-flight checks ──────────────────────────────────
command -v gh  >/dev/null 2>&1 || die "gh CLI is required. Install: https://cli.github.com/"
command -v git >/dev/null 2>&1 || die "git is required."
command -v cargo >/dev/null 2>&1 || die "cargo is required."

# Ensure working tree is clean
if [ -n "$(git status --porcelain)" ]; then
  die "Working tree is dirty. Commit or stash changes first."
fi

# Ensure on main branch
current_branch=$(git branch --show-current)
if [ "$current_branch" != "main" ]; then
  die "Must be on main branch (currently on: $current_branch)"
fi

# Ensure up to date with remote
git fetch origin main --quiet
local_sha=$(git rev-parse HEAD)
remote_sha=$(git rev-parse origin/main)
if [ "$local_sha" != "$remote_sha" ]; then
  die "Local main is not up to date with origin/main. Pull or push first."
fi

# ── Current version ─────────────────────────────────────
current_version=$(grep -m1 '^version' "$CARGO_TOML" | sed 's/.*"\(.*\)"/\1/')
info "Current version: v${current_version}"

IFS='.' read -r major minor patch <<< "$current_version"

# ── Determine last tag ──────────────────────────────────
last_tag=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
if [ -z "$last_tag" ]; then
  info "No previous tags found. This will be the first release."
  commit_range="HEAD"
else
  info "Last release: ${last_tag}"
  commit_range="${last_tag}..HEAD"
fi

# ── Collect commits since last release ──────────────────
commits=$(git log "$commit_range" --pretty=format:"%s" 2>/dev/null || echo "")
if [ -z "$commits" ]; then
  die "No commits since last release."
fi

echo ""
info "Commits since last release:"
git log "$commit_range" --pretty=format:"  - %s" 2>/dev/null
echo ""

# ── Auto-detect bump type from conventional commits ─────
detect_bump() {
  local has_breaking=false
  local has_feat=false

  while IFS= read -r msg; do
    if echo "$msg" | grep -qiE '^.*!:' || echo "$msg" | grep -qi 'BREAKING CHANGE'; then
      has_breaking=true
    elif echo "$msg" | grep -qi '^feat'; then
      has_feat=true
    fi
  done <<< "$commits"

  if $has_breaking; then
    echo "major"
  elif $has_feat; then
    echo "minor"
  else
    echo "patch"
  fi
}

bump_type="${1:-$(detect_bump)}"
info "Bump type: ${bump_type}"

# ── Calculate new version ───────────────────────────────
case "$bump_type" in
  major) major=$((major + 1)); minor=0; patch=0 ;;
  minor) minor=$((minor + 1)); patch=0 ;;
  patch) patch=$((patch + 1)) ;;
  *) die "Invalid bump type: $bump_type (use major|minor|patch)" ;;
esac

new_version="${major}.${minor}.${patch}"
new_tag="v${new_version}"
ok "New version: ${new_tag}"

# ── Confirm ─────────────────────────────────────────────
echo ""
read -rp "Proceed with release ${new_tag}? [y/N] " confirm
if [[ ! "$confirm" =~ ^[yY]$ ]]; then
  info "Aborted."
  exit 0
fi

# ── Update Cargo.toml ──────────────────────────────────
sed -i "s/^version = \"${current_version}\"/version = \"${new_version}\"/" "$CARGO_TOML"
ok "Updated ${CARGO_TOML} to ${new_version}"

# ── Update Cargo.lock ──────────────────────────────────
cargo generate-lockfile --quiet 2>/dev/null || cargo check --quiet 2>/dev/null || true
ok "Updated Cargo.lock"

# ── Commit and tag ─────────────────────────────────────
git add "$CARGO_TOML" Cargo.lock
git commit -m "chore: release ${new_tag}"
git tag -a "$new_tag" -m "Release ${new_tag}"
ok "Created commit and tag: ${new_tag}"

# ── Push ───────────────────────────────────────────────
info "Pushing to origin..."
git push origin main
git push origin "$new_tag"
ok "Pushed commit and tag"

# ── Wait for GitHub Actions ────────────────────────────
info "Waiting for Release workflow to start..."
sleep 5

# Find the workflow run for this tag
max_wait=300  # 5 minutes
elapsed=0
run_id=""

while [ $elapsed -lt $max_wait ]; do
  run_id=$(gh run list --repo "$REPO" --workflow=release.yml --limit=1 --json databaseId,headBranch \
    --jq ".[] | select(.headBranch == \"${new_tag}\") | .databaseId" 2>/dev/null || echo "")

  if [ -n "$run_id" ]; then
    break
  fi

  sleep 5
  elapsed=$((elapsed + 5))
done

if [ -z "$run_id" ]; then
  warn "Could not find workflow run after ${max_wait}s. Check GitHub Actions manually."
  warn "https://github.com/${REPO}/actions"
  exit 0
fi

info "Workflow run found: #${run_id}"
info "Waiting for build to complete..."

if gh run watch "$run_id" --repo "$REPO" --exit-status; then
  ok "Release workflow completed successfully!"
else
  die "Release workflow failed. Check: https://github.com/${REPO}/actions/runs/${run_id}"
fi

# ── Generate release notes ─────────────────────────────
info "Generating release notes..."

# Build release notes from commits
release_notes="## What's Changed\n\n"

while IFS= read -r msg; do
  release_notes+="- ${msg}\n"
done <<< "$commits"

if [ -n "$last_tag" ]; then
  release_notes+="\n**Full Changelog**: https://github.com/${REPO}/compare/${last_tag}...${new_tag}"
fi

# Update the GitHub release with notes
gh release edit "$new_tag" --repo "$REPO" --notes "$(echo -e "$release_notes")"
ok "Release notes updated"

echo ""
ok "Release ${new_tag} complete!"
info "https://github.com/${REPO}/releases/tag/${new_tag}"
