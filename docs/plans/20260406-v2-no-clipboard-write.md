# clipboard2path-wsl v2: クリップボード非書き換え設計

- **作成日**: 2026-04-06
- **ステータス**: planning
- **起源**: v1実装後のユーザーフィードバック — WSLg双方向同期によりwl-copyがWindows側クリップボードを破壊する問題

## 概要

WSL2上でクリップボードの画像をファイルとして保存し、パスを別経路（ファイル + シェルフック）で提供する軽量デーモン。
**クリップボードは一切書き換えない** — Windows側（Slack等）の画像貼り付けを壊さない。

## 背景: なぜ v2 が必要か

WSLgのクリップボード同期はRDP CLIPRDRチャネル経由の **双方向同期**。
`wl-copy` でWaylandクリップボードにテキストを書くと、Windows側クリップボードも上書きされる。
→ スクショをSlackに貼り付けたいのにパス文字列が貼られてしまう。

## 設計方針

1. **クリップボードは読み取り専用** — `wl-paste` のみ使用、`wl-copy` は使わない
2. **パス通知はファイル経由** — `$XDG_RUNTIME_DIR/clipboard2path/latest-path` に書き出し
3. **シェルフックでペースト統合** — `clipboard2path-wsl init` で fish/bash/zsh のペーストを拡張
4. **デーモンライフサイクル管理** — 起動時にディレクトリ作成、停止時にクリーンアップ

## 前提・制約

- `wl-paste --type image/bmp` ���動作確認済み（PNG直接取得は非対応）
- WSLg経由のWaylandクリップボード（RDP CLIPRDR双方向同期）
- `$XDG_RUNTIME_DIR` が存在する（systemd有効のWSL2環境）
- 動作の軽さ・バイナリサイズの小ささを最優先
- ドメインロジックは純粋関数、I/Oから分離（テスタビリティ最優先）

---

## Phase 1 — コア変更（クリップボード非書き換え化）

**目標**: 既存実装からwl-copy依存を除去し、ファイル経由のパス通知に切り替える

### Step 1.1: データディレクトリの管理
- [ ] `domain/runtime_dir.rs`: ランタイムディレクトリのパス解決（純粋関数）
  - `fn resolve_runtime_dir(xdg_runtime_dir: Option<&str>) -> Result<PathBuf, RuntimeDirError>`
  - `$XDG_RUNTIME_DIR/clipboard2path/` を返す
  - `$XDG_RUNTIME_DIR` 未設定時はエラー（明確なメッセージ付き）
  - テスト: 環境変数あり→正常パス、なし→エラー
- [ ] `domain/mod.rs` に `pub mod runtime_dir;` を追加
- [ ] `infra/lifecycle.rs`: デーモンライフサイクル（トレイト + 実装）
  ```rust
  pub trait DaemonLifecycle {
      fn setup(&self, dir: &Path) -> Result<(), LifecycleError>;    // ディレクトリ作成
      fn teardown(&self, dir: &Path) -> Result<(), LifecycleError>; // ディレクトリ削除
  }
  ```
  - setup: `mkdir -p` + パーミッション `0o700`
  - teardown: ディレクトリ内ファイル全削除 + `rmdir`
- [ ] `infra/mod.rs` に `pub mod lifecycle;` を追加
- [ ] `main.rs`: デーモン起動前に `DaemonLifecycle::setup()` を呼び出し
- [ ] SIGTERM/SIGINT ハンドリング: `ctrlc` クレートで teardown を実行
  - `Cargo.toml` に `ctrlc = "3"` を追加
  - `main.rs` で `ctrlc::set_handler` を登録し、teardown後に `process::exit(0)`

### Step 1.2: ClipboardWriter の除去 → PathNotifier の導入
- [ ] `infra/clipboard.rs` から `ClipboardWriter` トレイトと `WlClipboard` の `ClipboardWriter` 実装を削除
  - `MockWriter` 関連テストも削除
- [ ] `service/converter.rs` の `ConvertService` からジェネリクスパラメータ `W: ClipboardWriter` を除去
  - `ConvertService<C, W, F, T>` → `ConvertService<C, F, T, N>` (`N: PathNotifier`)
  - `new()` シグネチャから `clipboard_writer` を除去し、`path_notifier` を追加
  - `convert_once()` のステップ5: `self.clipboard_writer.write_text()` → `self.path_notifier.notify()`
- [ ] `service/daemon.rs` の `poll_once` ジェネリクスから `W: ClipboardWriter` を除去し、`N: PathNotifier` に置換
- [ ] `main.rs` の DI 組み立て（`ConvertService::new(...)` 呼び出し）を更新
  - `WlClipboard`（Writer用）を `FilePathNotifier` に置換
- [ ] `infra/path_notifier.rs`: パス通知（トレイト + 実装）
  ```rust
  pub trait PathNotifier {
      fn notify(&self, path: &Path) -> Result<(), NotifyError>;
  }
  ```
  - 実装: `FilePathNotifier` — `latest-path` ファイルに書き出し + `latest.png` シンボリックリンク更新
  - アトミック書き込み（一時ファイル → rename）: 一時ファイルも `0o600` パーミッションで作成
  - シンボリックリンク更新もアトミック: 一時名でsymlink作成 → `rename()` で差し替え
- [ ] `infra/mod.rs` に `pub mod path_notifier;` を追加
- [ ] `service/converter.rs` の `AppError` に `Notify(NotifyError)` バリアントを追加 + `From<NotifyError>` 実装

### Step 1.3: 自己トリガー防止の簡素化
- [ ] `wl-copy` を使わなくなったので、デバウンス機構は **不要になる可能性が高い**
  - クリップボードを書き換えないなら自己トリガーは発生しない
  - `domain/debounce.rs` を削除、`service/daemon.rs` の `poll_once` からデバウンスチェックを除去
  - テスト: デバウンス関連テストを削除、poll_onceのテストを更新

### Step 1.4: 保存先の変更
- [ ] デフォルト保存先: `/tmp/` → `$XDG_RUNTIME_DIR/clipboard2path/`
  - `domain/cli.rs` の `CliArgs::default()` で `output_dir` を `$XDG_RUNTIME_DIR/clipboard2path/` に変更
  - 注意: `$XDG_RUNTIME_DIR` が未設定の場合は `main.rs` で `resolve_runtime_dir()` を呼び、エラー時は明確なメッセージで終了
  - CLIデフォルト値は `PathBuf::from("")`（空 = 未指定を意味）とし、`main.rs` で `resolve_runtime_dir()` の結果をフォールバックに使用
- [ ] `--output-dir` オプションは引き続きサポート（明示指定で上書き可能）
- [ ] `help_text()` のデフォルト値表示を更新
- [ ] ファイル名形式は変更なし: `clipboard-{timestamp}.png`

### Step 1.5: ローテーション見直し
- [ ] `domain/cleanup.rs`: デフォルト保持上限を変更
  - 最大保持数: 100 → **20**
  - 年齢ベースクリーンアップ: 24時間 → **不要**（件数ベースのみで十分）
  - `files_to_clean_by_age()` 関数を削除（使用箇所なくなるため）
  - 最古のファイルから削除
- [ ] `main.rs` の `run_cleanup()` から年齢ベース削除ロジックを除去（件数ベースのみ残す）
- [ ] `--max-files` デフォルト値を `domain/cli.rs` で 20 に更新

### Step 1.6: テスト更新
- [ ] `infra/clipboard.rs` の `MockWriter` / `ClipboardWriter` 関連テストを削除
- [ ] `infra/path_notifier.rs` のテストを追加（`MockPathNotifier` + `FilePathNotifier` 統合テスト）
- [ ] `service/converter.rs` のテストを `PathNotifier` ベースに書き換え
  - `MockClipboardWriter` → `MockPathNotifier` に置換
  - `convert_once_writes_path_to_clipboard` → `convert_once_notifies_path` に書き換え
- [ ] `service/daemon.rs` のテストから `ClipboardWriter` 依存を除去、`PathNotifier` に置換
- [ ] `domain/debounce.rs` のテストを削除（ファイルごと削除するため）
- [ ] `domain/cleanup.rs` の `files_to_clean_by_age` 関連テストを削除
- [ ] `cargo test` + `cargo clippy` 全パス

**推定工数**: 3-4時間

---

## Phase 2 — シェル統合（`clipboard2path-wsl init`）

**目標**: ワンコマンドでシェルのペーストフックを設定し、以降は Ctrl+V だけで動く

### Step 2.1: シェル検出
- [ ] `domain/shell_detect.rs`: 現在のシェルを判定（純粋関数）
  - `fn detect_shell(shell_env: &str) -> ShellType`
  - `$SHELL` 環境変数から fish / bash / zsh を判定
  - テスト: 各シェルパス → 正しい ShellType
- [ ] `domain/mod.rs` に `pub mod shell_detect;` を追加

### Step 2.2: シェルフック生成
- [ ] `domain/shell_hook.rs`: シェルごとのフックスクリプト生成（純粋関数）
  - `fn generate_hook(shell: ShellType, runtime_dir: &Path) -> String`
  - **fish**: `fish_clipboard_paste` 関数の上書き
    ```fish
    function fish_clipboard_paste
        set -l latest_path "$XDG_RUNTIME_DIR/clipboard2path/latest-path"
        if test -f "$latest_path"; and wl-paste --list-types 2>/dev/null | string match -q '*image/bmp*'
            commandline -i -- (string trim -- (cat "$latest_path"))
        else
            commandline -i -- (wl-paste -n 2>/dev/null)
        end
    end
    ```
    - `string trim` で末尾改行を除去し、パスに余分な文字が混入しないようにする
  - **bash**: `readline` のペーストバインド
  - **zsh**: `zle` ウィジェットのペーストバインド
  - テスト: 各シェルタイプ → 期待するスクリプト文字列を含む
- [ ] `domain/mod.rs` に `pub mod shell_hook;` を追加

### Step 2.3: CLI サブコマンド対応
- [ ] `domain/cli.rs` をサブコマンド対応に拡張
  - 現在のフラットなオプションパーサーから、サブコマンド構造に変更:
    ```rust
    pub enum Command {
        Watch(WatchArgs),     // デフォルト — 従来のデーモン/単発モード
        Init(InitArgs),       // シェルフックのインストール
        Uninstall,            // シェルフックの削除
    }
    ```
  - 既存の `--once`, `--interval` 等のオプションは `Watch` サブコマンドの引数として維持
  - サブコマンドなしの場合は `Watch` として動作（後方互換性）
  - テスト: サブコマンドパース、後方互換テスト

### Step 2.4: init サブコマンド実装
- [ ] `clipboard2path-wsl init [fish|bash|zsh]`
  - シェル指定なし → 自動検出
  - フックスクリプトを適切な場所に配置:
    - fish: `~/.config/fish/functions/fish_clipboard_paste.fish`
    - bash: `~/.bashrc` に `source` 行を追記
    - zsh: `~/.zshrc` に `source` 行を追記
  - 既存設定がある場合は確認メッセージ表示（上書きしない）
  - `--force` で強制上書き
- [ ] `clipboard2path-wsl uninstall` — フックを削除

### Step 2.5: テスト
- [ ] シェル検出テスト
- [ ] フック生成テスト（各シェル）
- [ ] CLIサブコマンドパーステスト（後方互換含む）
- [ ] initコマンドの統合テスト（モック注入でファイル書き込み検証）

**推定工数**: 3-4時間

---

## Phase 3 — 品質・最適化

**目標**: バイナリサイズ最適化、ドキュメント更新

### Step 3.1: バイナリサイズ最適化
- [ ] release profile は既存のまま（`opt-level = "z"`, `lto = true`, `strip = true`）
- [ ] `wl-copy` 関連コード削除によるサイズ削減を確認
- [ ] 目標: 600KB以下（v1: 593KB、wl-copy削除分で微減期待）

### Step 3.2: systemd サービス更新
- [ ] `clipboard2path.service` を更新
  - `ExecStop` で teardown（SIGTERM でクリーンアップ）を確認
  - `Environment=XDG_RUNTIME_DIR=/run/user/%U` を確認

### Step 3.3: README 更新
- [ ] 新しいセットアップ手順（`clipboard2path-wsl init` の説明）
- [ ] クリップボード非書き換えの設計思想を記載
- [ ] Windows 側への影響がないことを明記

### Step 3.4: Wiki 更新
- [ ] アーキテクチャ概要の更新（ClipboardWriter削除、PathNotifier追加）
- [ ] WSL2 クリップボード連携記事の更新（双方向同期の問題と対策）

**推定工数**: 2-3時���

---

## リスクと対策

| リスク | 影響 | 対策 |
|--------|------|------|
| `$XDG_RUNTIME_DIR` 未設定 | 起動不可 | 明確なエラーメッセージ + `--output-dir` フォールバック |
| tmpfs メモリ圧迫 | システム不安定 | 保持上限20件でローテーション |
| シェルフック競合 | 既存ペースト動作が壊れる | init時に既存設定を検出し確認、`uninstall` で復元可能 |
| SIGTERM 未受信 | teardown 未実行 | tmpfs なのでOS再起動で消える（最悪ケースでも安全） |

## 成功基準

- [ ] Phase 1: クリップボード非書き換えで画像→PNG→パスファイル更新が動作する
- [ ] Phase 2: `clipboard2path-wsl init` 後、Ctrl+V でパスが入力される。Slack等のWindows貼り付けが壊れない
- [ ] Phase 3: README + Wiki が更新され、新設計が文書化されている
