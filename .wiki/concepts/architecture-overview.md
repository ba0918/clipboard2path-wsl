---
title: "アーキテクチャ概要"
type: wiki
source_refs:
  - raw/articles/architecture-overview.md
created: 2026-04-06
updated: 2026-04-06
category: architecture
tags: [architecture, rust, di, pure-functions, layers, testing]
related:
  - "[[なぜ clipboard2path-wsl が必要なのか]]"
  - "[[WSL2 クリップボード連携の技術詳細]]"
---

# アーキテクチャ概要

## 概要

clipboard2path-wsl は **テスタビリティ最優先** の3層アーキテクチャで構成される。ドメインロジックは全て純粋関数、外部依存はトレイトで抽象化し DI 可能、上位層はビジネスロジックゼロで呼び出しのみ。

**v2 の最大の変更点**: クリップボードは読み取り専用。パス通知はファイル経由 + シェルフック。

## レイヤー構成

```
┌─────────────────────────────────────────┐
│  main.rs -- DI Assembly + Routing       │  エントリポイント
│  (サブコマンド分岐 + 実装の組み立て)       │
├─────────────────────────────────────────┤
│  Service Layer -- Orchestration         │  フロー制御
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

## ドメイン層

全て `fn(input) -> output` 形式の純粋関数。外部状態の読み書きなし。

| モジュール | 関数 | 役割 |
|-----------|------|------|
| `image_convert` | `convert_bmp_to_png(&[u8]) -> Result<Vec<u8>>` | BMP->PNG変換 |
| `path_gen` | `generate_save_path(&Path, &str) -> Result<PathBuf>` | 保存パス生成 |
| `path_gen` | `validate_output_dir(&Path) -> Result<PathBuf>` | パストラバーサル防止 |
| `wsl_detect` | `is_wsl2(&str) -> bool` | WSL2環境判定 |
| `runtime_dir` | `resolve_runtime_dir(Option<&str>) -> Result<PathBuf>` | XDGランタイムディレクトリ解決 |
| `clipboard_change` | `has_clipboard_changed(&[String], &[String]) -> bool` | 変更検知 |
| `clipboard_change` | `has_bmp_image(&[String]) -> bool` | BMP存在チェック |
| `cleanup` | `files_to_clean_by_count(...)` | 件数ベース一時ファイル管理 |
| `cli` | `parse_args(&[String]) -> Result<Command>` | CLI引数パース（サブコマンド対応） |
| `shell_detect` | `detect_shell(&str) -> Result<ShellType>` | シェル検出 |
| `shell_hook` | `generate_hook(ShellType) -> String` | シェルフック生成 |

## インフラ層

トレイトで抽象化し、テスト時はモック差し替え：

```rust
pub trait ClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError>;
    fn list_types(&self) -> Result<Vec<String>, ClipboardError>;
}

pub trait PathNotifier {
    fn notify(&self, path: &Path) -> Result<(), NotifyError>;
}

pub trait FileWriter {
    fn write_bytes(&self, path: &Path, data: &[u8]) -> Result<(), FsError>;
}

pub trait DaemonLifecycle {
    fn setup(&self, dir: &Path) -> Result<(), LifecycleError>;
    fn teardown(&self, dir: &Path) -> Result<(), LifecycleError>;
}
```

**v2 で削除**: `ClipboardWriter` (wl-copy 依存を完全除去)
**v2 で追加**: `PathNotifier`, `DaemonLifecycle`, `ShellInstaller`

**セキュリティ対策**:
- `Command::arg()` 直接渡し（シェルインジェクション防止）
- ファイルパーミッション `0o600`、ディレクトリ `0o700`
- アトミック書き込み（temp file -> rename）

## サービス層

### ConvertService

ジェネリクスで全依存を注入：

```rust
pub struct ConvertService<C, F, T, N>
where C: ClipboardReader, F: FileWriter,
      T: TimestampProvider, N: PathNotifier
```

`convert_once()` が変換パイプラインを制御（BMP読取 -> PNG変換 -> 保存 -> パス通知）。ビジネスロジックゼロ。

### daemon::poll_once

ポーリングの1イテレーションを関数化：

```
型リスト取得 -> 変更検知 -> BMP確認 -> 変換
```

v2 ではデバウンスチェックを削除（クリップボード書き換えしないため自己トリガーが発生しない）。

## main.rs

DI コンテナの組み立てとサブコマンドルーティング：

```rust
let notifier = FilePathNotifier::new(base_dir.clone());
let service = ConvertService::new(
    WlClipboard, RealFileWriter, SystemTimestamp, notifier
);
```

サブコマンド: `Watch`（デフォルト）、`Init`、`Uninstall`、`Help`、`Version`

## テスト戦略

**90 テスト**、全てユニットテスト。外部コマンド依存はインフラ層に閉じ込めてあるため CI でも全テスト動作。

## バイナリ最適化

```toml
[profile.release]
opt-level = "z"    # サイズ最優先
lto = true         # リンク時最適化
strip = true       # シンボル除去
codegen-units = 1  # 単一生成ユニット
panic = "abort"    # unwinding 削減
```

`image` crate は `features = ["bmp", "png"]` のみ。リリースバイナリ約 670KB。

## 関連項目

- [[なぜ clipboard2path-wsl が必要なのか]] -- プロジェクトの動機と背景
- [[WSL2 クリップボード連携の技術詳細]] -- wl-paste の技術詳細
