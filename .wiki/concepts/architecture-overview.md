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

## レイヤー構成

```
┌─────────────────────────────────────────┐
│  main.rs — DI Assembly                  │  エントリポイント
│  (実装の組み立て + モード分岐のみ)        │
├─────────────────────────────────────────┤
│  Service Layer — Orchestration          │  フロー制御
│  ├── converter.rs  変換パイプライン       │
│  └── daemon.rs     ポーリングループ       │
├──────────────────┬──────────────────────┤
│  Domain Layer    │  Infra Layer         │
│  (pure functions)│  (I/O via traits)    │
│  ├ image_convert │  ├ clipboard.rs      │
│  ├ path_gen      │  └ file_system.rs    │
│  ├ wsl_detect    │                      │
│  ├ clipboard_chg │                      │
│  ├ debounce      │                      │
│  ├ cleanup       │                      │
│  └ cli           │                      │
└──────────────────┴──────────────────────┘
```

**依存の方向は上から下のみ。** ドメイン層はインフラ層に依存しない。

## ドメイン層（48 テスト）

全て `fn(input) -> output` 形式の純粋関数。外部状態の読み書きなし。

| モジュール | 関数 | 役割 |
|-----------|------|------|
| `image_convert` | `convert_bmp_to_png(&[u8]) -> Result<Vec<u8>>` | BMP→PNG変換 |
| `path_gen` | `generate_save_path(&Path, &str) -> Result<PathBuf>` | 保存パス生成 |
| `path_gen` | `validate_output_dir(&Path) -> Result<PathBuf>` | パストラバーサル防止 |
| `wsl_detect` | `is_wsl2(&str) -> bool` | WSL2環境判定 |
| `debounce` | `should_process_event(Option<u64>, u64, u64) -> bool` | 自己トリガー防止 |
| `clipboard_change` | `has_clipboard_changed(&[String], &[String]) -> bool` | 変更検知 |
| `clipboard_change` | `has_bmp_image(&[String]) -> bool` | BMP存在チェック |
| `cleanup` | `files_to_clean_by_age(...)` / `..._by_count(...)` | 一時ファイル管理 |
| `cli` | `parse_args(&[String]) -> Result<CliArgs>` | CLI引数パース |

## インフラ層（6 テスト）

トレイトで抽象化し、テスト時はモック差し替え：

```rust
pub trait ClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError>;
    fn list_types(&self) -> Result<Vec<String>, ClipboardError>;
}

pub trait ClipboardWriter {
    fn write_text(&self, text: &str) -> Result<(), ClipboardError>;
}

pub trait FileWriter {
    fn write_bytes(&self, path: &Path, data: &[u8]) -> Result<(), FsError>;
}
```

**セキュリティ対策**:
- `Command::arg()` 直接渡し（シェルインジェクション防止）
- ファイルパーミッション `0o600`

## サービス層（9 テスト）

### ConvertService

ジェネリクスで全依存を注入：

```rust
pub struct ConvertService<C, W, F, T>
where C: ClipboardReader, W: ClipboardWriter,
      F: FileWriter, T: TimestampProvider
```

`convert_once()` が変換パイプラインを制御（BMP読取 → PNG変換 → 保存 → パス通知）。ビジネスロジックゼロ。

### daemon::poll_once

ポーリングの1イテレーションを関数化：

```
デバウンスチェック → 型リスト取得 → 変更検知 → BMP確認 → 変換
```

各段階で早期リターンにより不要処理を回避。

## main.rs

DI コンテナの組み立てとモード分岐のみ：

```rust
let service = ConvertService::new(
    WlClipboard, WlClipboard, RealFileWriter, SystemTimestamp
);
```

## テスト戦略

**63 テスト**、全てユニットテスト。外部コマンド依存はインフラ層に閉じ込めてあるため CI でも全テスト動作。

| レイヤー | テスト数 | 手法 |
|---------|---------|------|
| Domain | 48 | 入力→出力の純粋な検証 |
| Infra | 6 | トレイト契約のモック検証 |
| Service | 9 | モック注入でフロー検証 |

## バイナリ最適化

```toml
[profile.release]
opt-level = "z"    # サイズ最優先
lto = true         # リンク時最適化
strip = true       # シンボル除去
codegen-units = 1  # 単一生成ユニット
panic = "abort"    # unwinding 削減
```

`image` crate は `features = ["bmp", "png"]` のみ。リリースバイナリ約 593KB。

## 関連項目

- [[なぜ clipboard2path-wsl が必要なのか]] — プロジェクトの動機と背景
- [[WSL2 クリップボード連携の技術詳細]] — wl-paste/wl-copy の技術詳細
