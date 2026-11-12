#!/usr/bin/env bash
# Local checks -> version bump -> commit, tag, push to GitHub.
# Nothing is built here: the tag starts .github/workflows/release.yml, which builds,
# signs, notarises and publishes StatusBadge.dmg in a GitHub release.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BUMP=patch
PUSH=1
for arg in "$@"; do
    case "$arg" in
        --no-push) PUSH=0 ;;
        --help|-h)
            echo 'Usage: ./release.sh [current|patch|minor|major|X.Y.Z] [--no-push]'
            echo 'Default: run the checks, choose the next patch version, commit, tag and push.'
            echo 'The GitHub workflow started by the tag builds, signs, notarises and publishes the DMG.'
            echo 'current tags the version already in Cargo.toml (first publication).'
            echo '--no-push keeps the commit and the tag local.'
            exit 0 ;;
        --*) echo "Unknown option: $arg" >&2; exit 2 ;;
        *) BUMP="$arg" ;;
    esac
done
if [ -n "$(git status --porcelain)" ]; then echo 'Commit changes before releasing.' >&2; exit 1; fi
BRANCH="$(git branch --show-current)"
if [ "$BRANCH" != main ] && [ "$BRANCH" != master ]; then echo 'Release from main or master.' >&2; exit 1; fi
for tool in cargo python3; do
    command -v "$tool" >/dev/null || { echo "Missing local tool: $tool" >&2; exit 1; }
done
# The workflow only runs on GitHub: a tag pushed to another remote releases nothing.
REMOTE="${RELEASE_REMOTE:-$(git remote -v | awk '/github\.com[:\/]/ && /\(push\)/ { print $1; exit }')}"
if [ "$PUSH" = 1 ]; then
    if [ -z "$REMOTE" ]; then
        echo 'No Git remote points to github.com. Add it, or pass RELEASE_REMOTE=<remote>.' >&2; exit 1
    fi
    git fetch "$REMOTE" --tags
    echo 'Vérification de l’accès Git (simulation, aucun push à cette étape).'
    git push --dry-run "$REMOTE" "$BRANCH"
fi
VERSION="$(python3 - "$BUMP" <<'PY'
import pathlib, re, subprocess, sys, tomllib
current = tomllib.loads(pathlib.Path('Cargo.toml').read_text())['workspace']['package']['version']
tags = subprocess.check_output(['git', 'tag', '--list', 'v*'], text=True).splitlines()
versions = [tuple(map(int, tag[1:].split('.'))) for tag in tags
            if re.fullmatch(r'v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', tag)]
major, minor, patch = max([tuple(map(int, current.split('.'))), *versions])
bump = sys.argv[1]
if bump == 'current': new = current
elif bump == 'patch': new = f'{major}.{minor}.{patch+1}'
elif bump == 'minor': new = f'{major}.{minor+1}.0'
elif bump == 'major': new = f'{major+1}.0.0'
else: new = bump
if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', new): raise SystemExit('Invalid version')
if bump != 'current' and tuple(map(int, new.split('.'))) <= (major, minor, patch):
    raise SystemExit('Version must increase (use current for the first publication)')
print(new)
PY
)"
if git rev-parse --verify "refs/tags/v$VERSION" >/dev/null 2>&1; then
    echo "Tag v$VERSION already exists; choose a new version." >&2; exit 1
fi
echo "Releasing v$VERSION."
python3 - "$VERSION" <<'PY'
import pathlib, re, sys, tomllib
path = pathlib.Path('Cargo.toml')
text = path.read_text()
current = tomllib.loads(text)['workspace']['package']['version']
new = sys.argv[1]
path.write_text(text.replace(f'version = "{current}"', f'version = "{new}"', 1))
path = pathlib.Path('bundle/Info.plist')
text = path.read_text()
for key in ('CFBundleShortVersionString', 'CFBundleVersion'):
    text = re.sub(r'(<key>' + key + r'</key>\s*<string>)[^<]+', lambda m: m[1] + new, text)
path.write_text(text)
PY
cargo update --workspace
python3 bundle/verify-version.py "v$VERSION"
# A failed check leaves the version change uncommitted and no tag: fix, `git checkout .`, rerun.
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
git add Cargo.toml Cargo.lock bundle/Info.plist
if ! git diff --cached --quiet; then git commit -m "release v$VERSION"; fi
git tag -a "v$VERSION" -m "v$VERSION"
if [ "$PUSH" = 0 ]; then
    echo "v$VERSION committed and tagged locally. To publish: git push --atomic ${REMOTE:-<github remote>} $BRANCH v$VERSION"
    exit 0
fi
git push --atomic "$REMOTE" "$BRANCH" "v$VERSION"
echo "v$VERSION pushed to $REMOTE. The release workflow builds, signs, notarises and publishes the DMG."
echo 'Suivi : gh run watch'
