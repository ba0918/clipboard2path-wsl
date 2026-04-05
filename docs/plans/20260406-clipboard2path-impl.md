# clipboard2path-wsl 実装計画

- **作成日**: 2026-04-06
- **ステータス**: 🔵 implementing
- **起源**: [チーム壁打ち](../ideas/001-implementation-strategy.md)

## 概要

WSL2上でクリップボードの画像をファイルとして保存し、そのパスをクリップボードにセットする軽量デーモン。
`wl-paste --type image/bmp` → Rust内BMP→PNG変換 → ファイル保存 → `wl-copy` でパス通知。

## 前提・制約

- `wl-paste --type image/bmp` が動作確認済み（PNG直接取得は非対応）
- WSLg経由のWaylandクリップボードを使用
- 動作の軽さ・バイナリサイズの小ささを最優先
- ドメインロジックは純粋関数、I/Oから分離（テスタビリティ最優先）

---

## Phase 1 — MVP（最小動作プロダクト）

**目標**: 単発実行で「クリップボード画像 → PNGファイル → パスをクリップボードにコピー」が動く

### Step 1.1: プロジェクト初期化
- [ ] `cargo init` でRustプロジェクト作成
- [ ] `Cargo.toml` に依存追加: `image`（BMP→PNG変換）
- [ ] モジュール構成を作成:
  ```
  src/
    main.rs          — エントリポイント（DI組み立て + Service呼び出しのみ）
    domain/
      mod.rs
      image_convert.rs  — BMP→PNG変換（純粋関数）
      path_gen.rs       — 保存先パス生成 + パス検証（純粋関数）
      wsl_detect.rs     — WSL2環境判定（純粋関数）
    service/
      mod.rs
      converter.rs      — 変換オーケストレーション（ドメイン呼び出し + I/O委譲）
    infra/
      mod.rs
      clipboard.rs      — wl-paste/wl-copy 呼び出し（トレイト + 実装）
      file_system.rs    — ファイル保存（トレイト + 実装）
  ```

### Step 1.2: ドメイン層実装（純粋関数 + テスト）
- [ ] `image_convert.rs`: `fn convert_bmp_to_png(bmp_bytes: &[u8]) -> Result<Vec<u8>, ConvertError>`
  - `image` crateでBMPデコード → PNGエンコード
  - テスト: 有効なBMP → PNG変換成功、不正データ → エラー、空データ → エラー
- [ ] `path_gen.rs`: `fn generate_save_path(base_dir: &Path, timestamp: &str) -> PathBuf`
  - `/tmp/clipboard-{timestamp}.png` 形式のパス生成
  - **パス検証**: `base_dir` のカノニカライズ + `..` トラバーサル検出で安全なパスのみ許可
  - テスト: タイムスタンプ入力 → 期待するパス出力、パストラバーサル試行 → エラー
- [ ] `path_gen.rs`: `fn validate_output_dir(path: &Path) -> Result<PathBuf, PathError>`
  - カノニカライズ + ディレクトリ存在確認 + 書き込み権限チェック
  - テスト: 正常パス → Ok、`..` 含むパス → エラー、存在しないパス → エラー
- [ ] `wsl_detect.rs`: `fn is_wsl2(proc_version: &str) -> bool`
  - `/proc/version` の内容から "microsoft" or "WSL" を判定
  - テスト: WSL2文字列 → true、通常Linux → false

### Step 1.3: インフラ層実装（トレイト + 実装）
- [ ] `clipboard.rs`:
  ```rust
  pub trait ClipboardReader {
      fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError>;
  }
  pub trait ClipboardWriter {
      fn write_text(&self, text: &str) -> Result<(), ClipboardError>;
  }
  ```
  - 実装: `WlClipboard` — `wl-paste --type image/bmp` / `wl-copy` を呼び出し
  - **セキュリティ**: `wl-copy` 呼び出し時は `Command::new("wl-copy").arg(path)` でシェル経由せず直接引数渡し（コマンドインジェクション防止）
  - **セキュリティ**: `wl-paste` の戻り値を信頼しない — BMP ヘッダ検証をドメイン層で実施
- [ ] `file_system.rs`:
  ```rust
  pub trait FileWriter {
      fn write_bytes(&self, path: &Path, data: &[u8]) -> Result<(), IoError>;
  }
  ```
  - **パーミッション**: 保存ファイルは `0o600`（所有者のみ読み書き可）で作成
  - テスト: ファイル作成後のパーミッション検証

### Step 1.35: Service層実装
- [ ] `converter.rs`: `ConvertService` 構造体（トレイトオブジェクト注入）
  ```rust
  pub struct ConvertService<C: ClipboardReader, W: ClipboardWriter, F: FileWriter> {
      clipboard_reader: C,
      clipboard_writer: W,
      file_writer: F,
  }
  impl<C, W, F> ConvertService<C, W, F> {
      pub fn convert_once(&self, base_dir: &Path) -> Result<PathBuf, AppError> { ... }
  }
  ```
  - ドメイン関数（BMP→PNG変換、パス生成）を呼び出し、I/Oをインフラ層に委譲
  - テスト: モックインフラ注入で全フロー検証（単体テスト可能）

### Step 1.4: main.rs（エントリポイント）
- [ ] DI組み立て: 実インフラ実装を `ConvertService` に注入
- [ ] WSL2判定 → 非WSLなら `exit(1)` + エラーメッセージ
- [ ] `ConvertService::convert_once()` 呼び出し
- [ ] **エラーハンドリング方針**: `Result` 型を `main()` まで伝搬し、`process::exit()` でexit codeを返す
  - 成功: exit 0
  - クリップボードに画像なし: exit 0（正常系、stderr にメッセージ）
  - 変換エラー / I/Oエラー: exit 1 + stderr にアクショナブルなメッセージ
- [ ] ビジネスロジックゼロ — DI組み立てとService呼び出しのみ

### Step 1.5: 動作確認
- [ ] `cargo build` → `cargo test` → `cargo clippy` 全パス
- [ ] 実際にクリップボードに画像コピー → 実行 → PNGファイル生成 → パスがクリップボードに入ること確認

**推定工数**: 3-4時間

---

## Phase 2 — デーモン化・堅牢化

**目標**: バックグラウンドで常駐し、クリップボード変更を検知して自動変換

### Step 2.1: クリップボード監視実装
- [ ] **方式選定**: `wl-paste --watch` の利用可否を調査
  - 利用可能: イベント駆動方式（`wl-paste --watch --type image/bmp` のstdoutを監視）→ ポーリング不要で効率的
  - 利用不可/不安定: フォールバックとしてポーリング方式を採用
- [ ] ポーリング方式の場合: 一定間隔（デフォルト500ms）でクリップボード監視
- [ ] **軽量差分検出**: BMP全量ハッシュではなく、`wl-paste --list-types` でMIMEタイプ一覧を取得し、変更の有無を軽量判定。変更検知時のみBMP全量取得。
- [ ] **自己トリガー防止**: テキスト書き込み直後のクリップボード変更は無視（フラグ + タイムスタンプでデバウンス）

### Step 2.2: エラーハンドリング強化
- [ ] `wl-paste` 未インストール時の明確なエラーメッセージ（「wl-paste が見つかりません。`sudo apt install wl-clipboard` でインストールしてください」）
- [ ] クリップボードに画像がない場合（テキスト等）のスキップ（エラーではなく正常系、debugログ出力）
- [ ] ファイル書き込み失敗時のリトライ（最大3回、1秒間隔）+ エラーログ
- [ ] ディスク容量不足時の明確なエラーメッセージ

### Step 2.25: 一時ファイルクリーンアップ
- [ ] デーモン起動時に古い一時PNGファイルを削除（デフォルト: 24時間以上前のファイル）
- [ ] `--max-files <N>` オプション: 保持する最大ファイル数（デフォルト: 100）
- [ ] クリーンアップはサービス層の責務として実装

### Step 2.3: CLI引数
- [ ] `--once`: 単発実行モード（Phase 1相当）
- [ ] `--interval <ms>`: ポーリング間隔指定
- [ ] `--output-dir <path>`: 保存先ディレクトリ指定
- [ ] `--help` / `--version`

### Step 2.4: systemd user service
- [ ] `clipboard2path.service` ファイル作成
- [ ] インストール手順をREADMEに記載
- [ ] `ExecStart`, `Restart=on-failure` 設定

### Step 2.5: テスト強化
- [ ] ポーリングループの統合テスト（モック注入）
- [ ] 自己トリガー防止のテスト
- [ ] エラーケースのテスト

**推定工数**: 4-6時間

---

## Phase 3 — 最適化・拡張

**目標**: バイナリサイズ最小化、ユーザビリティ向上

### Step 3.1: バイナリサイズ最適化
- [ ] `Cargo.toml` のrelease profile: `opt-level = "z"`, `lto = true`, `strip = true`
- [ ] 不要なfeatureの除外（`image` crateのBMP/PNGのみ有効化）
- [ ] 目標: 500KB以下

### Step 3.2: 設定ファイル対応
- [ ] `~/.config/clipboard2path/config.toml`
- [ ] ポーリング間隔、保存先、ファイル名パターン等

### Step 3.3: ログ出力
- [ ] `--verbose` / `--quiet` フラグ
- [ ] 変換成功/失敗のログ

### Step 3.4: 将来検討（スコープ外メモ）
- イベント駆動（`wl-clipboard-rs` crate）への切り替え
- JPEG出力対応
- 逆変換モード（WSLパス → Windowsパス）
- フックAPI（変換後に任意スクリプト実行）

**推定工数**: 3-5時間

---

## リスクと対策

| リスク | 影響 | 対策 |
|--------|------|------|
| WSLg環境差異 | 動作不安定 | `/proc/version` 判定 + `wl-paste` 存在チェックで早期フェイル |
| 自己トリガー無限ループ | CPU暴走 | Phase 2で最優先実装。書き込み直後フラグで防止 |
| `image` crateのサイズ | バイナリ肥大化 | feature flags でBMP/PNGのみ有効化 |
| クリップボードロック競合 | 取得失敗 | リトライ + エラースキップ |

## 成功基準

- [ ] Phase 1: 単発実行で画像→PNG→パスの変換が動作する
- [ ] Phase 2: デーモンとして常駐し自動変換できる
- [ ] Phase 3: リリースバイナリ 500KB以下
