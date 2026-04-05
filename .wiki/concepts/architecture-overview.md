---
title: "アーキテクチャ概要"
type: wiki
source_refs:
  - raw/articles/architecture-overview.md
created: 2026-04-06
updated: 2026-04-06
category: architecture
tags: [architecture, rust, di, pure-functions, layers, v2, path-notifier]
related:
  - "[[なぜ clipboard2path-wsl が必要なのか]]"
  - "[[WSL2 クリップボード連携の技術詳細]]"
---

# アーキテクチャ概要（v2）

## 概要

clipboard2path-wsl は **テスタビリティ最優先** の3層アーキテクチャ。v2 ではクリップボードを読み取り専用とし、パス通知をファイル経由に変更。90 テスト、全て純粋関数またはモック注入で検証。

## レイヤー構成

```
┌─────────────────────────────────────────┐
│  main.rs — DI Assembly + Routing        │
│  (サブコマンド分岐 + 実装の組み立て)       │
├─────────────────────────────────────────┤
│  Service Layer — Orchestration          │
│  ├── converter.rs  変換パイプライン       │
│  └── daemon.rs     ポーリングループ       │
├──────────────────┬──────────────────────┤
│  Domain Layer    │  Infra Layer         │
│  (pure functions)│  (I/O via traits)    │
│  ├ image_convert │  ├ clipboard.rs      │
│  ├ path_gen      │  ├ file_system.rs    │
│  ├ wsl_detect    │  ├ path_notifier.rs  │
│  ├ clipboard_chg │  ├ lifecycle.rs      │
│  ├ runtime_dir   │  └ shell_installer.rs│
│  ├ cleanup       │                      │
│  ├ cli           │                      │
│  ├ shell_detect  │                      │
│  └ shell_hook    │                      │
└──────────────────┴──────────────────────┘
```

**依存の方向は上から下のみ。** ドメイン層はインフラ層に依存しない。

## v1 → v2 変更サマリー

| 項目 | v1 | v2 |
|------|-----|-----|
| クリップボード | 読み書き（wl-paste + wl-copy） | **読み取り専用** |
| パス通知 | `ClipboardWriter` | **`PathNotifier`**（latest-path + symlink） |
| 保存先 | `/tmp/` | **`$XDG_RUNTIME_DIR/clipboard2path/`** |
| 自己トリガー防止 | `debounce.rs` | **削除**（書き換えないので不要） |
| ローテーション | 100件 + 24時間 | **20件**（件数ベースのみ） |
| CLI構造 | フラット | **サブコマンド**（Watch/Init/Uninstall） |
| シェル統合 | なし | **fish/bash/zsh フック** |
| ライフサイクル | なし | **setup/teardown**（SIGTERM対応） |
| テスト | 63 | **90** |
| バイナリ | 593KB | **670KB** |

## ドメイン層

全て `fn(input) -> output` 形式の純粋関数。

| モジュール | 主要関数 | 役割 |
|-----------|---------|------|
| `image_convert` | `convert_bmp_to_png(&[u8])` | BMP→PNG変換 |
| `path_gen` | `generate_save_path`, `validate_output_dir` | パス生成 + トラバーサル防止 |
| `wsl_detect` | `is_wsl2(&str)` | WSL2環境判定 |
| `runtime_dir` | `resolve_runtime_dir(Option<&str>)` | XDG ランタイムディレクトリ解決 |
| `clipboard_change` | `has_clipboard_changed`, `has_bmp_image` | 変更検知 |
| `cleanup` | `files_to_clean_by_count` | 件数ベースローテーション |
| `cli` | `parse_args(&[String])` | サブコマンドパース |
| `shell_detect` | `detect_shell(&str)` | シェル検出 |
| `shell_hook` | `generate_hook(ShellType)` | フックスクリプト生成 |

## インフラ層

```rust
// 読み取り専用（v2 で ClipboardWriter を削除）
pub trait ClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError>;
    fn list_types(&self) -> Result<Vec<String>, ClipboardError>;
}

// v2 新設: ファイル経由パス通知
pub trait PathNotifier {
    fn notify(&self, path: &Path) -> Result<(), NotifyError>;
}

// v2 新設: デーモンライフサイクル
pub trait DaemonLifecycle {
    fn setup(&self, dir: &Path) -> Result<(), LifecycleError>;
    fn teardown(&self, dir: &Path) -> Result<(), LifecycleError>;
}

pub trait FileWriter { ... }
```

**セキュリティ**:
- ファイルパーミッション `0o600`、ディレクトリ `0o700`
- [[WSL2 クリップボード連携の技術詳細|アトミック書き込み]]（temp → rename）

## サービス層

### ConvertService

```rust
pub struct ConvertService<C, F, T, N>
where C: ClipboardReader, F: FileWriter,
      T: TimestampProvider, N: PathNotifier
```

変換パイプライン: BMP読取 → PNG変換 → ファイル保存 → **パス通知**

### daemon::poll_once

```
型リスト取得 → 変更検知 → BMP確認 → 変換
```

デバウンスチェック不要（v2 ではクリップボード書き換えしない）。

## main.rs

```rust
let notifier = FilePathNotifier::new(base_dir.clone());
let service = ConvertService::new(WlClipboard, RealFileWriter, SystemTimestamp, notifier);
```

サブコマンド: `Watch`（デフォルト）、`Init`、`Uninstall`、`Help`、`Version`

## テスト戦略

**90 テスト**。全てユニットテスト。外部コマンド依存はインフラ層に閉じ込め CI 動作可能。

## バイナリ最適化

```toml
[profile.release]
opt-level = "z"    # サイズ最優先
lto = true         # リンク時最適化
strip = true       # シンボル除去
codegen-units = 1  # 単一生成ユニット
panic = "abort"    # unwinding 削減
```

`image` crate `features = ["bmp", "png"]` のみ + `ctrlc` crate。約 670KB。

## 関連項目

- [[なぜ clipboard2path-wsl が必要なのか]] — v2 設計変更の動機
- [[WSL2 クリップボード連携の技術詳細]] — wl-paste、シェルフック、アトミック更新の詳細
