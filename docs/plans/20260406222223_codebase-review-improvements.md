# コードベースレビュー改善（88点→目標95点）

**Cycle ID:** `20260406222223`
**Started:** 2026-04-06 22:22:23
**Status:** 🟢 Complete

---

## What & Why

コードベースレビュー（88点/Aランク）で発見された MEDIUM 6件 + 主要 LOW 8件を一括改善する。
CRITICAL/HIGH はゼロのため、防御的プログラミングの強化・コード衛生・パフォーマンス微最適化が中心。

## Goals

- MEDIUM 指摘 6件を全て解消
- 主要 LOW 指摘 8件を全て解消
- 既存テスト 146 個を壊さない
- clippy / rustfmt クリーン維持

## Design

### Step 1: パス埋め込みのセキュリティ強化（MEDIUM x2）

**対象:**
- `src/domain/systemd_unit.rs:10` — `exec_path` バリデーション（defense-in-depth: `current_exe().canonicalize()` 由来の信頼済みパスだが、防御的に検証）
- `src/domain/shell_hook.rs:91` — `hook_file` バリデーション（`source "{hook_file}"` のダブルクォート内インジェクション防止）

**変更内容:**
- `src/domain/path_validate.rs` を新規作成（純粋関数モジュール）
- `src/domain/mod.rs` に `pub mod path_validate;` を追加
  - `validate_safe_path(path: &str) -> Result<(), PathError>` — 以下の文字を検出してエラー:
    - シェル特殊文字: `"`, `$`, `` ` ``, `\`, `'`
    - systemd specifier: `%`
    - 制御文字: `0x00-0x1F`（改行 `\n`, キャリッジリターン `\r` を含む）
  - `is_safe_for_shell(path: &str) -> bool` — 上記のブール版
  - **設計判断: 拒否リスト（denylist）方式を採用。** 理由: Linux パスは UTF-8 を含む広範な文字を許容するため、許可リスト方式はパスの多様性を不必要に制限する。既知のリスク: 将来の攻撃ベクトル（Unicode 正規化等）には対応できない可能性があるが、現時点では生成先が systemd unit と shell source 行に限定されており、上記の文字セットで十分な防御を提供する。
- `generate_unit()` の先頭で `validate_safe_path(exec_path)?` を呼び出す
  - 戻り値を `Result<String, PathError>` に変更
  - エラーメッセージ: 「バイナリパスに予期しない文字が含まれています。パスを確認してください: {path}」
- `generate_source_line()` の先頭で `validate_safe_path(hook_file)?` を呼び出す
  - 戻り値を `Result<String, PathError>` に変更
- `main.rs` の呼び出し側でエラーハンドリング追加

**テスト:**
- `validate_safe_path` に正常パス / 各種特殊文字パスのテスト
- `validate_safe_path` に制御文字（`\n`, `\r`, `\0`）のテスト
- `validate_safe_path` に `%` のテスト
- `generate_unit` / `generate_source_line` のエラーケーステスト
- 既存テストが `unwrap()` → `?` に対応

### Step 2: ポーリングバッファ再利用（MEDIUM x1）

**対象:**
- `src/service/daemon.rs:31-40` — `poll_once` が毎回 `Vec<String>` を返す
- `src/main.rs:467` — ポーリングループ

**変更内容:**
- `poll_once` のシグネチャを変更:
  ```rust
  // Before
  pub fn poll_once<C, F, T, N>(
      service: &ConvertService<C, F, T, N>,
      previous_types: &[String],
      base_dir: &Path,
  ) -> (PollResult, Vec<String>)
  // After
  pub fn poll_once<C, F, T, N>(
      service: &ConvertService<C, F, T, N>,
      previous_types: &mut Vec<String>,
      base_dir: &Path,
  ) -> PollResult
  ```
- 変更検知後: `previous_types.clear()` + `previous_types.extend(new_types)`
- エラー時: `previous_types` をそのまま保持（`to_vec()` クローン排除）
- `main.rs` のループ: `let (result, new_types) = ...` → `let result = ...`
- **注記:** パフォーマンスインパクトは軽微（MIMEタイプは数個、500ms間隔のポーリング）。この変更はコードベースレビュー指摘への対応であり、API の明確化（所有権の意図をシグネチャで表現）が主目的。

**テスト:**
- 既存の `poll_once` テスト3件を新シグネチャに適合
- バッファが再利用されていることを確認するテスト追加

### Step 3: validate_output_dir の infra 層移動（MEDIUM x1）

**対象:**
- `src/domain/path_gen.rs:52-69` — `validate_output_dir` が `canonicalize()` / `is_dir()` でI/Oアクセス

**変更内容:**
- `src/domain/path_gen.rs` から `validate_output_dir` を削除
- `src/infra/file_system.rs` に `validate_output_dir` を移動（I/O操作はinfra層の責務）
- domain層には `validate_path_components(path: &Path) -> Result<(), PathError>` を残す（`..` 検出の純粋関数部分のみ）
- `PathError` 型は `src/domain/path_gen.rs` に残留（domain層の型定義）。infra層は `use crate::domain::path_gen::PathError` で参照する（上位→下位の依存方向を維持）
- `main.rs` の呼び出し元を `infra::file_system::validate_output_dir` に変更

**テスト:**
- `path_gen.rs` のテスト → `validate_path_components` 用に簡素化
- `file_system.rs` に `validate_output_dir` のテスト移動（tmpdir で実FS使用）

### Step 4: Option<PathBuf> 型安全化（MEDIUM x1）

**対象:**
- `src/domain/cli.rs:48-58` — `output_dir: PathBuf::from("")`
- `src/main.rs:346` — `args.output_dir.as_os_str().is_empty()` 判定

**変更内容:**
- `WatchArgs.output_dir` を `Option<PathBuf>` に変更
- `Default` 実装: `output_dir: None`
- `cli.rs` のパース: `"--output-dir"` で `Some(PathBuf::from(val))` を設定
- `main.rs:346`: `match args.output_dir { None => runtime_dir, Some(dir) => validate... }`
- help テキスト更新

**影響を受ける既存テスト（全て `cli.rs` 内）:**
- `default_args_returns_watch` — `assert_eq!(w.output_dir, PathBuf::from(""))` → `assert_eq!(w.output_dir, None)` に変更
- `output_dir_flag` — `PathBuf::from("/home/user/images")` → `Some(PathBuf::from("/home/user/images"))` に変更
- `combined_watch_flags` — output_dir 未指定のため `None` との比較に変更不要

**テスト:**
- 上記の既存 CLI テストを `Option<PathBuf>` に適合
- `None` / `Some` 両方のケースをカバー

### Step 5: doc comment 修正 + wl-copy 言及削除（MEDIUM x1 + LOW x1）

**対象:**
- `src/infra/clipboard.rs:7` — `ClipboardError::ToolNotFound` の doc comment に `wl-copy` への言及（プロジェクト根幹要件違反）
- 6モジュールの `//!` doc comment 欠落

**変更内容:**
- `clipboard.rs` の doc comment から `wl-copy` 言及を削除
- 以下のモジュールに `//!` doc comment 追加:
  - `domain/path_gen.rs` — `//! File path generation and validation (pure functions).`
  - `domain/image_convert.rs` — `//! BMP to PNG image conversion (pure functions).`
  - `domain/wsl_detect.rs` — `//!` に変換（現在 `///` を使用）
  - `domain/mod.rs` — `//! Domain layer: pure business logic with no I/O dependencies.`
  - `infra/clipboard.rs` — `//! Clipboard reading via wl-paste (read-only).`
  - `infra/file_system.rs` — `//! File system write abstraction.`
  - `infra/mod.rs` — `//! Infrastructure layer: I/O implementations behind trait abstractions.`
  - `service/converter.rs` — `//! Clipboard-to-file conversion service.`
  - `service/daemon.rs` — `//! Daemon poll loop logic.`
  - `service/mod.rs` — `//! Service layer: domain + infra orchestration.`

### Step 6: CLI バリデーション強化（LOW x2）

**対象:**
- `src/domain/cli.rs:137` — `--interval` 範囲チェックなし
- `src/domain/cli.rs:154` — `--max-files` 下限チェックなし

**変更内容:**
- 定数定義:
  ```rust
  pub const MIN_INTERVAL_MS: u64 = 100;
  pub const MAX_INTERVAL_MS: u64 = 60_000;
  pub const DEFAULT_INTERVAL_MS: u64 = 500;
  pub const MIN_MAX_FILES: usize = 1;
  pub const DEFAULT_MAX_FILES: usize = 20;
  ```
- パース後にバリデーション: `CliError::OutOfRange { flag, min, max, actual }`
- `Default` 実装とヘルプテキストで定数を使用（DRY）

**テスト:**
- `--interval 0` → エラー
- `--interval 99` → エラー
- `--interval 100` → OK
- `--interval 60001` → エラー
- `--max-files 0` → エラー
- `--max-files 1` → OK

### Step 7: コード重複解消 + リファクタリング（LOW x4）

**対象:**
- テストモック重複（converter.rs / daemon.rs）
- `which wl-paste` コード重複（main.rs:114 / 263）
- `detect_shell` / `parse_shell_name` 重複（shell_detect.rs）
- PNG バッファ容量ヒント未指定（image_convert.rs）

**変更内容:**

a) **テストモック共通化:**
- `src/service/test_helpers.rs` を新規作成
- `src/service/mod.rs` に `#[cfg(test)] pub(crate) mod test_helpers;` を追加（テスト時のみコンパイル、クレート内公開）
- `MockClipboardReader`, `MockFileWriter`, `MockPathNotifier`, `FixedTimestamp`, `make_1x1_bmp` を移動
- `converter.rs` と `daemon.rs` のテストから `use super::test_helpers::*;` でインポート

b) **which wl-paste ヘルパー:**
- `main.rs` に `fn resolve_wl_paste_path() -> Option<String>` を追加
- 2箇所の重複を呼び出しに置換

c) **shell_detect 共通化リファクタリング:**
- `detect_shell` 内でベースネーム抽出後に `parse_shell_name(basename)` を呼び出す形に変更（マッチロジックの共通化）

d) **PNG バッファ容量ヒント:**
- `image_convert.rs:36`: `Vec::new()` → `Vec::with_capacity(bmp_bytes.len())`

### Step 8: シェルフックパーミッション明示化（LOW x1）

**対象:**
- `src/infra/shell_installer.rs:99, 118` — `std::fs::write()` でパーミッション未指定

**変更内容:**
- fish フック / bash/zsh フックファイルの書き込み後に `std::fs::set_permissions(path, Permissions::from_mode(0o644))` を追加
- フックファイルは `source` で読み込むだけで実行しないため `0o644` が適切

**テスト:**
- 既存のシェルインストーラーテストにパーミッション検証を追加

## Tests

### 新規テスト
- [ ] `validate_safe_path` — 正常パス（英数字、`/`、`-`、`_`、`.`）
- [ ] `validate_safe_path` — 各種特殊文字（`"`, `$`, `` ` ``, `\`, `'`）でエラー
- [ ] `validate_safe_path` — 制御文字（`\n`, `\r`, `\0`）でエラー
- [ ] `validate_safe_path` — systemd specifier `%` でエラー
- [ ] `generate_unit` — 特殊文字パスでエラー
- [ ] `generate_source_line` — 特殊文字パスでエラー
- [ ] `poll_once` — バッファ再利用の確認
- [ ] `validate_path_components` — `..` 検出（domain層の純粋関数）
- [ ] `validate_output_dir` — infra層での実FS テスト
- [ ] CLI `--interval 0` → OutOfRange
- [ ] CLI `--interval 99` → OutOfRange
- [ ] CLI `--interval 60001` → OutOfRange
- [ ] CLI `--max-files 0` → OutOfRange
- [ ] シェルフックファイルのパーミッション検証

### 既存テスト変更
- [ ] `generate_unit` テスト — `Result` unwrap 対応
- [ ] `generate_source_line` テスト — `Result` unwrap 対応
- [ ] `poll_once` テスト3件 — `&mut Vec<String>` シグネチャ対応
- [ ] `WatchArgs` / CLI テスト — `Option<PathBuf>` 対応
- [ ] `validate_output_dir` テスト — infra 層に移動

## Security

- [ ] シェルスクリプト埋め込みパスのバリデーション関数はドメイン層（純粋関数）
- [ ] `validate_safe_path` は拒否リスト（denylist）方式を採用:
  - 対象文字: `"`, `$`, `` ` ``, `\`, `'`, `%`, 制御文字 (0x00-0x1F)
  - 理由: Linux パスは UTF-8 含む広範な文字を許容し、許可リスト方式では正当なパスを誤拒否するリスクが高い
  - 既知の制限: Unicode 正規化攻撃には未対応（現在のユースケースでは低リスク）
- [ ] 既存のパストラバーサル防御は維持

## Progress

| Step | Description | Status |
|------|-------------|--------|
| 1 | パス埋め込みセキュリティ強化 | 🟢 |
| 2 | ポーリングバッファ再利用 | 🟢 |
| 3 | validate_output_dir の infra 層移動 | 🟢 |
| 4 | Option<PathBuf> 型安全化 | 🟢 |
| 5 | doc comment 修正 + wl-copy 言及削除 | 🟢 |
| 6 | CLI バリデーション強化 | 🟢 |
| 7 | コード重複解消 + リファクタリング | 🟢 |
| 8 | シェルフックパーミッション明示化 | 🟢 |

**Legend:** ⚪ Pending · 🟡 In Progress · 🟢 Done

---

**Next:** Refine → Implement → Commit 🚀
