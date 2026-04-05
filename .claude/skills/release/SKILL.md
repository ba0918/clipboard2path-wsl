---
name: release
description: 前回リリースからの変更を分析し、バージョン自動判定 → Cargo.toml 更新 → tag push → CD 待ち → LLM によるリリースノート生成を一括実行する。「release」「リリース」で起動。
---

# Release

前回リリースからの変更内容を分析し、セマンティックバージョニングに基づくリリースを一括実行するスキル。
LLM がコミット履歴とコード差分を読み取り、人間が読みやすいリリースノートを自動生成する。

## 引数

- `$ARGUMENTS` が `major` / `minor` / `patch` の場合: bump タイプを強制指定
- 引数なし: conventional commits から自動判定

## 定数

- **REPO**: `ba0918/clipboard2path-wsl`
- **BINARY_NAME**: `clipboard2path-wsl`
- **CARGO_TOML**: `Cargo.toml`

## Phase 1: Pre-flight Check

以下を **すべて** 確認し、1つでも失敗したら理由を伝えて **中止**:

1. `gh` CLI が使用可能か
2. `git status --porcelain` が空（working tree がクリーン）か
3. 現在のブランチが `main` か
4. `origin/main` と同期済みか（`git fetch origin main` → SHA 比較）

## Phase 2: 変更分析

1. 現在のバージョンを `Cargo.toml` から取得
2. 最新タグを `git describe --tags --abbrev=0` で取得（タグなしなら初回リリース）
3. 前回タグ以降のコミットを取得:
   ```
   git log {last_tag}..HEAD --pretty=format:"%h %s" (タグあり)
   git log --pretty=format:"%h %s" (タグなし)
   ```
4. 前回タグ以降のコード差分を取得:
   ```
   git diff {last_tag}..HEAD --stat
   ```
5. コミットがゼロなら「リリースする変更がありません」と伝えて **中止**

## Phase 3: バージョン決定

`$ARGUMENTS` で bump タイプが指定されていればそれを使用。未指定なら自動判定:

| コミットパターン | bump |
|---|---|
| `BREAKING CHANGE` または `*!:` | major |
| `feat:` / `feat(` | minor |
| それ以外 | patch |

現在バージョンから新バージョンを算出し、タグ名 `v{new_version}` を決定する。

## Phase 4: リリースノート生成

前回タグ以降の **コミットメッセージ** と **diff stat** を元に、以下の構造でリリースノートを生成する:

```markdown
## Highlights

（このリリースの要点を 1〜3 文で。ユーザー目線で何が変わったかを伝える）

## Changes

- **カテゴリ**: 変更の要約（関連コミット: abc1234, def5678）

## Notes

（破壊的変更、マイグレーション手順、既知の問題があれば記載。なければセクションごと省略）
```

### リリースノート生成のルール

- コミットメッセージをそのまま羅列しない。意味のある単位にグルーピングして要約する
- ユーザーが読んで「何が変わったか」が分かる粒度にする
- `chore:` / `docs:` / `ci:` のみの変更はまとめて軽く触れる程度でよい
- 技術的な内部リファクタリングは「内部改善」として簡潔にまとめる
- 日本語で記述する

## Phase 5: ユーザー確認

以下を表示してユーザーに確認を求める（AskUserQuestion を使用）:

```
リリース内容:
  バージョン: v{current} → v{new}
  bump: {major|minor|patch}
  コミット数: {n}

リリースノート:
{生成したリリースノート}

このまま実行してよい？
```

ユーザーが承認しなかった場合、フィードバックに応じて修正するか中止する。

## Phase 6: 実行

承認後、以下を **順番に** 実行する:

1. `Cargo.toml` の `version` を新バージョンに更新（Edit ツール使用）
2. `cargo generate-lockfile` で `Cargo.lock` を更新
3. `git add Cargo.toml Cargo.lock`
4. `git commit -m "chore: release v{new_version}"`
5. `git tag -a "v{new_version}" -m "Release v{new_version}"`
6. `git push origin main`
7. `git push origin "v{new_version}"`

## Phase 7: CD 待ち & リリースノート更新

1. 5秒待ってから `gh run list --repo {REPO} --workflow=release.yml --limit=1` でワークフロー実行を検出
2. `gh run watch {run_id} --repo {REPO} --exit-status` で完了を待つ（Bash の timeout を 600000 に設定）
3. 成功したら `gh release edit "v{new_version}" --repo {REPO} --notes "{リリースノート}"` でノートを更新
4. 失敗したら GitHub Actions の URL を伝えて対処を促す

## Phase 8: 完了レポート

```
Release v{new_version} 完了!
https://github.com/{REPO}/releases/tag/v{new_version}
```

## 禁止事項

- `--no-verify` の使用
- `git push --force`
- main 以外のブランチからのリリース
- ユーザー確認なしでの tag push
