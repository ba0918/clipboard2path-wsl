---
title: "アーキテクチャ概要"
category: architecture
tags: [architecture, rust, di, pure-functions, layers, v2]
source_type: internal
updated: 2026-04-06
---

# アーキテクチャ概要（v2）

## 設計原則

- **テスタビリティ最優先** — 全設計判断はテスト容易性から逆算
- **クリップボード非書き換え** — `wl-paste` のみ使用、Windows 側に影響ゼロ
- ドメインロジックは全て純粋関数（I/O なし、副作用なし）
- 外部依存はトレイトで抽象化し、DI（依存注入）可能
- 上位層はビジネスロジックゼロ

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

依存の方向は上から下のみ。ドメイン層はインフラ層に依存しない。

## v1 → v2 の変更

| 項目 | v1 | v2 |
|------|-----|-----|
| クリップボード | 読み書き（wl-paste + wl-copy） | **読み取り専用**（wl-paste のみ） |
| パス通知 | ClipboardWriter（wl-copy） | **PathNotifier**（latest-path + symlink） |
| 保存先 | /tmp/ | **$XDG_RUNTIME_DIR/clipboard2path/** |
| 自己トリガー防止 | debounce.rs | **不要**（書き換えないので発生しない） |
| ローテーション | 100件 + 24時間 | **20件のみ**（件数ベース） |
| CLI構造 | フラット | **サブコマンド**（Watch/Init/Uninstall） |
| シェル統合 | なし | **fish/bash/zsh フック** |
| ライフサイクル | なし | **setup/teardown**（SIGTERM 対応） |

## ドメイン層（純粋関数）

| モジュール | 関数 | 役割 |
|-----------|------|------|
| `image_convert` | `convert_bmp_to_png(&[u8]) -> Result<Vec<u8>>` | BMP→PNG変換 |
| `path_gen` | `generate_save_path(&Path, &str) -> Result<PathBuf>` | 保存パス生成 |
| `path_gen` | `validate_output_dir(&Path) -> Result<PathBuf>` | パストラバーサル防止 |
| `wsl_detect` | `is_wsl2(&str) -> bool` | WSL2環境判定 |
| `runtime_dir` | `resolve_runtime_dir(Option<&str>) -> Result<PathBuf>` | XDG ランタイムディレクトリ解決 |
| `clipboard_change` | `has_clipboard_changed(&[String], &[String]) -> bool` | 変更検知 |
| `clipboard_change` | `has_bmp_image(&[String]) -> bool` | BMP存在チェック |
| `cleanup` | `files_to_clean_by_count(...)` | 件数ベース一時ファイル管理 |
| `cli` | `parse_args(&[String]) -> Result<Command>` | CLI引数パース（サブコマンド対応） |
| `shell_detect` | `detect_shell(&str) -> Result<ShellType>` | シェル検出 |
| `shell_hook` | `generate_hook(ShellType) -> String` | シェルフック生成 |

## インフラ層（トレイト抽象化）

```rust
// 読み取り専用（v2で ClipboardWriter を削除）
pub trait ClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError>;
    fn list_types(&self) -> Result<Vec<String>, ClipboardError>;
}

// v2 で新設: ファイル経由パス通知
pub trait PathNotifier {
    fn notify(&self, path: &Path) -> Result<(), NotifyError>;
}

pub trait FileWriter {
    fn write_bytes(&self, path: &Path, data: &[u8]) -> Result<(), FsError>;
}

// v2 で新設: デーモンライフサイクル
pub trait DaemonLifecycle {
    fn setup(&self, dir: &Path) -> Result<(), LifecycleError>;
    fn teardown(&self, dir: &Path) -> Result<(), LifecycleError>;
}
```

**FilePathNotifier** の実装:
- `latest-path` ファイルへのアトミック書き込み（temp → rename、`0o600`）
- `latest.png` シンボリックリンクのアトミック更新（temp symlink → rename）

**FsDaemonLifecycle** の実装:
- setup: `mkdir -p` + `0o700`
- teardown: ファイル全削除 + `rmdir`

## サービス層

### ConvertService

```rust
pub struct ConvertService<C, F, T, N>
where C: ClipboardReader, F: FileWriter,
      T: TimestampProvider, N: PathNotifier
```

変換パイプライン: BMP読取 → PNG変換 → ファイル保存 → **パス通知**（latest-path + symlink）

### daemon::poll_once

```
型リスト取得 → 変更検知 → BMP確認 → 変換
```

v2 ではデバウンスチェックが不要（クリップボード書き換えしないため自己トリガーなし）。

## main.rs

DI 組み立て + サブコマンドルーティング:

```rust
let notifier = FilePathNotifier::new(base_dir.clone());
let service = ConvertService::new(WlClipboard, RealFileWriter, SystemTimestamp, notifier);
```

サブコマンド: `Watch`（デフォルト）、`Init`、`Uninstall`、`Help`、`Version`

## テスト戦略

90 テスト。外部コマンド依存はインフラ層に閉じ込めてあるため CI でも全テスト動作。

## バイナリ最適化

`image` crate は `features = ["bmp", "png"]` のみ。`ctrlc` crate 追加。リリースバイナリ約 670KB。
