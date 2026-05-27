---
name: release-app
description: Use when publishing a new codex-shim or Codex Shim Control release, especially after README, version, Tauri app, provider, or release-note changes
---

# Release App

Use this for codex-shim project releases. The release has two surfaces:

- Python package / CLI: `pyproject.toml`, root `README.md`, `CHANGELOG.md`.
- Tauri desktop app: `tauri-app/package.json`, `tauri-app/package-lock.json`,
  `tauri-app/src-tauri/Cargo.toml`, `tauri-app/src-tauri/tauri.conf.json`,
  `tauri-app/README.md`, generated DMG bundle.

## Workflow

1. Sync and inspect state.

```bash
git fetch origin --tags
git status --short --branch
git tag --sort=-v:refname
git log --oneline --decorate -6
```

Do not stage unrelated untracked files such as screenshots. If the branch is not
on `main`, confirm the intended release branch before publishing.

2. Pick the version.

- Patch: docs-only or narrowly compatible fixes.
- Minor: new provider support, standalone app behavior, release bundle changes,
  or compatibility-impacting Tauri/Rust service changes.
- For this repo, update both CLI and app versions unless the user explicitly
  requests only one surface.

Files to update for an app release:

```text
pyproject.toml
tauri-app/package.json
tauri-app/package-lock.json
tauri-app/src-tauri/Cargo.toml
tauri-app/src-tauri/tauri.conf.json
CHANGELOG.md
README.md
tauri-app/README.md
```

3. Write release notes in both Chinese and English.

`CHANGELOG.md` should have a version section with:

- `### 中文`
- `#### 新增`
- `#### 优化` or `#### 修复`
- `#### 验证`
- `### English`
- `#### Added`
- `#### Changed` or `#### Fixed`
- `#### Verified`

Also update `tauri-app/README.md` recent changes and root `README.md` when the
release changes installation, runtime behavior, provider support, or app scope.

4. Verify before commit.

Run the full release gate when publishing the Tauri app:

```bash
python3.11 -m compileall codex_shim -q
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3.11 -m pytest -p pytest_asyncio.plugin tests/ -q
cargo check --offline
cargo test --offline
npm run build
npm run tauri:build
git diff --check
```

Run Rust commands from `tauri-app/src-tauri`; run npm commands from
`tauri-app`. Use offline Cargo commands unless a dependency update genuinely
requires network. `npm run tauri:build` writes the DMG under:

```text
tauri-app/src-tauri/target/release/bundle/dmg/
```

5. Commit and tag.

Stage only release metadata/docs and intended code changes:

```bash
git add CHANGELOG.md README.md pyproject.toml tauri-app/README.md \
  tauri-app/package-lock.json tauri-app/package.json \
  tauri-app/src-tauri/Cargo.toml tauri-app/src-tauri/tauri.conf.json
git commit -m "chore: release vX.Y.Z"
git tag -a vX.Y.Z -m "vX.Y.Z"
```

If `Cargo.lock` is still untracked/ignored, do not force-add it unless the
project policy changes.

6. Push and publish GitHub release.

```bash
git push origin main
git push origin vX.Y.Z
```

Create a temporary bilingual notes file outside the repo, for example:

```text
/private/tmp/codex-shim-vX.Y.Z-release-notes.md
```

Then publish:

```bash
gh release create vX.Y.Z \
  "tauri-app/src-tauri/target/release/bundle/dmg/Codex Shim Control_X.Y.Z_x64.dmg" \
  --title "vX.Y.Z" \
  --notes-file /private/tmp/codex-shim-vX.Y.Z-release-notes.md
```

GitHub normalizes spaces in asset names; verify the resulting asset name rather
than assuming it remains byte-for-byte identical.

7. Final verification.

```bash
git status --short --branch
git log --oneline --decorate -6
git ls-remote --tags origin vX.Y.Z
gh release view vX.Y.Z --json tagName,name,url,isDraft,isPrerelease,assets
```

Report the commit, tag, release URL, uploaded asset, verification commands, and
any remaining untracked files.

## Failure Handling

- If a command fails due to sandbox/network restrictions, retry with the normal
  approval flow instead of working around it.
- If `gh release create` succeeds but `gh release view` fails from network, keep
  the returned release URL and retry `gh release view` with approval.
- If validation fails, fix the failing cause and rerun the failed command plus
  any downstream command that depends on its output.
- Never claim the release is complete until the tag, remote branch, release, and
  asset have been verified.
