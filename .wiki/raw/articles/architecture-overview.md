---
title: "アーキテクチャ概要"
category: architecture
tags: [architecture, rust, di, pure-functions, layers]
source_type: internal
---

# アーキテクチャ概要

## 設計原則

clipboard2path-wsl は **テスタビリティ最優先** の設計原則に基づく。全ての設計判断はテスト容易性から逆算されている。

- ドメインロジックは全て純粋関数（I/O なし、副作用なし）
- 外部依存はトレイトで抽象化し、DI（依存注入）可能
- 上位層は下位層を呼び出すのみ、ビジネスロジックゼロ

## レイヤー構成

```
main.rs (DI Assembly)
  ↓
Service Layer (orchestration)
  ├── converter.rs   — 変換フロー制御
  └── daemon.rs      — ポーリングループ
  ↓
Domain Layer (pure functions)          Infra Layer (I/O)
  ├── image_convert.rs  BMP→PNG変換    ├── clipboard.rs   wl-paste/wl-copy
  ├── path_gen.rs       パス生成        └── file_system.rs ファイル書き込み
  ├── wsl_detect.rs     WSL2判定
  ├── clipboard_change.rs 変更検知
  ├── debounce.rs       自己トリガー防止
  ├── cleanup.rs        一時ファイル管理
  └── cli.rs            CLI引数パース
```

依存の方向は **上から下のみ**。ドメイン層はインフラ層に依存しない。サービス層がドメイン関数を呼び出し、インフラトレイトを通じて I/O を実行する。

## ドメイン層: 純粋関数

全て `fn(input) -> output` 形式。外部状態の読み書きなし。

### image_convert

```rust
pub fn convert_bmp_to_png(bmp_bytes: &[u8]) -> Result<Vec<u8>, ConvertError>
```

`image` crate でBMPデコード → PNGエンコード。入力バリデーション（空データチェック、BMP ヘッダ検証）をドメイン層で実施。

### path_gen

```rust
pub fn generate_save_path(base_dir: &Path, timestamp: &str) -> Result<PathBuf, PathError>
pub fn validate_output_dir(path: &Path) -> Result<PathBuf, String>
```

パス生成とパストラバーサル防止。出力ディレクトリをカノニカライズして検証。

### wsl_detect

```rust
pub fn is_wsl2(proc_version: &str) -> bool
```

`/proc/version` の文字列を受け取り判定。ファイル読み込み自体は呼び出し側の責務。

### debounce

```rust
pub fn should_process_event(last_write_ms: Option<u64>, current_ms: u64, debounce_ms: u64) -> bool
```

タイムスタンプ比較のみ。時計の巻き戻しにも対応。

### clipboard_change

```rust
pub fn has_clipboard_changed(previous: &[String], current: &[String]) -> bool
pub fn has_bmp_image(types: &[String]) -> bool
```

MIME タイプリストの比較。前回と異なれば変更ありと判定。

## インフラ層: トレイト抽象化

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

実装クラス `WlClipboard` は `Command::new("wl-paste")` / `Command::new("wl-copy")` を内部で呼び出す。テスト時はモック実装を注入。

セキュリティ対策:
- `wl-copy` への引数は `Command::arg()` で直接渡し（シェルインジェクション防止）
- ファイル書き込みはパーミッション `0o600`（owner のみ読み書き可能）

## サービス層: オーケストレーション

### ConvertService

```rust
pub struct ConvertService<C, W, F, T>
where C: ClipboardReader, W: ClipboardWriter, F: FileWriter, T: TimestampProvider
```

ジェネリクスで全依存を注入。`convert_once()` メソッドが変換フロー全体を制御：

1. クリップボードから BMP 読み取り
2. BMP → PNG 変換（ドメイン関数呼び出し）
3. ファイル保存（インフラトレイト呼び出し）
4. パスをクリップボードに書き込み（インフラトレイト呼び出し）

ビジネスロジックゼロ — 関数呼び出しの順序制御のみ。

### daemon::poll_once

```rust
pub fn poll_once<C, W, F, T>(
    service: &ConvertService<C, W, F, T>,
    previous_types: &[String],
    last_write_ms: Option<u64>,
    current_ms: u64,
    debounce_ms: u64,
    base_dir: &Path,
) -> (PollResult, Vec<String>)
```

ポーリングループの1イテレーションを関数化。デバウンスチェック → 型リスト取得 → 変更検知 → BMP 有無確認 → 変換実行の5段階パイプライン。

## main.rs: DI 組み立てのみ

```rust
let service = ConvertService::new(WlClipboard, WlClipboard, RealFileWriter, SystemTimestamp);
```

main.rs は DI コンテナの組み立てとモード分岐（`--once` vs デーモン）のみ。ドメイン関数もインフラ関数も直接呼ばない。

## テスト戦略

- **63 テスト**、全てユニットテスト
- ドメイン層: 純粋関数なので入力→出力の検証のみ（48 テスト）
- インフラ層: トレイトの契約をモックで検証（6 テスト）
- サービス層: モック注入でフロー全体をテスト（9 テスト）
- 外部コマンド（wl-paste/wl-copy）への依存はインフラ層に閉じ込められているため、CI でも全テストが動作する

## バイナリ最適化

```toml
[profile.release]
opt-level = "z"      # サイズ最優先最適化
lto = true           # リンク時最適化
strip = true         # デバッグシンボル除去
codegen-units = 1    # 単一コード生成ユニット
panic = "abort"      # パニック時即終了（unwinding コスト削減）
```

`image` crate は `default-features = false, features = ["bmp", "png"]` でBMP/PNG のみ有効化。リリースバイナリ約 593KB。
