# clipboard2path-wsl

[English](README.md) | 日本語

WSL2 上でクリップボードの画像を自動的にファイル保存し、シェルフック経由でパスを入力できるようにする軽量デーモン。

**クリップボードは一切書き換えない** -- Windows 側（Slack 等）の画像貼り付けを壊さない設計。

## 問題

WSL2 環境ではクリップボードに保存された画像を直接貼り付けられないケースがある。
WSLg のクリップボード同期は RDP CLIPRDR チャネル経由の双方向同期のため、`wl-copy` で
Wayland クリップボードに書き込むと Windows 側クリップボードも上書きされてしまう。

`clipboard2path-wsl` はクリップボードを **読み取り専用** で使用し、パスはファイル経由で
シェルフックに提供することで、この問題を解決する。

## 仕組み

1. クリップボードを監視（ポーリング、`wl-paste` のみ使用）
2. 画像（BMP）を検知したら取得
3. PNG に変換して `$XDG_RUNTIME_DIR/clipboard2path/` に保存
4. `latest-path` ファイルと `latest.png` シンボリックリンクを更新
5. シェルの Alt+V フックが `latest-path` を読み取りパスを入力
6. wl-paste ラッパー（`~/.local/bin/wl-paste`）が `image/png` 要求に `latest.png` を返し、Claude Code 等の画像ペーストを可能にする

## 必要要件

- WSL2 (WSLg 有効)
- `wl-paste` (`wl-clipboard` パッケージ)
- Rust toolchain (ビルド時のみ)

```bash
# Ubuntu/Debian
sudo apt install wl-clipboard
```

## インストール

### ワンライナー（推奨）

```bash
curl -fsSL https://raw.githubusercontent.com/ba0918/clipboard2path-wsl/main/scripts/install.sh | bash
```

`~/.local/bin` にバイナリを配置する。`INSTALL_DIR` で変更可能:

```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/ba0918/clipboard2path-wsl/main/scripts/install.sh | bash
```

### ソースからビルド

```bash
git clone https://github.com/ba0918/clipboard2path-wsl.git
cd clipboard2path-wsl
cargo install --path .
```

## セットアップ

`init` 一発でシェルフック + systemd サービス + wl-paste ラッパーがすべて設置され、デーモンも自動起動する:

```bash
# 自動検出（$SHELL から判定）
clipboard2path-wsl init

# シェルを明示指定
clipboard2path-wsl init fish
clipboard2path-wsl init bash
clipboard2path-wsl init zsh
```

シェルを再読み込み（新しいターミナルを開く）した以降、クリップボードに画像があるときに
Alt+V を押すとファイルパスが入力される。テキストがクリップボードにあるときはそのテキストを入力する。

状態確認:

```bash
clipboard2path-wsl status
```

## 使い方

### デーモンモード（デフォルト）

```bash
clipboard2path-wsl
```

クリップボードを 500ms 間隔で監視し、画像を検知するたびに
`$XDG_RUNTIME_DIR/clipboard2path/clipboard-{timestamp}.png` に保存する。

### 単発実行

```bash
clipboard2path-wsl --once
```

### セットアップ管理

```bash
clipboard2path-wsl init [fish|bash|zsh]         # インストール（フック + サービス + ラッパー）
clipboard2path-wsl init --force [fish|bash|zsh] # 強制上書き
clipboard2path-wsl uninstall [fish|bash|zsh]    # アンインストール（フック + サービス + ラッパー除去）
clipboard2path-wsl status                       # 状態表示
```

### オプション

```
COMMANDS:
    (default)           クリップボード監視（デーモンモード）
    init [SHELL]        シェルフックと systemd サービスをインストール
    uninstall [SHELL]   シェルフックと systemd サービスをアンインストール
    status              現在の状態を表示

WATCH OPTIONS:
    --once              単発実行（デーモンループなし）
    --interval <ms>     ポーリング間隔 ms（100-60000、デフォルト: 500）
    --output-dir <path> 出力ディレクトリ（デフォルト: $XDG_RUNTIME_DIR/clipboard2path/）
    --max-files <n>     保持する最大ファイル数（最小: 1、デフォルト: 20）
    --verbose           詳細ログ出力
    -q, --quiet         エラー以外の出力を抑制

INIT OPTIONS:
    -f, --force         既存フックを強制上書き
    --no-service        systemd サービスのインストールをスキップ

UNINSTALL OPTIONS:
    --no-service        systemd サービスの削除をスキップ

GLOBAL OPTIONS:
    -h, --help          ヘルプ表示
    -v, --version       バージョン表示
```

### 例

```bash
# 1秒間隔で監視、保存先を明示指定
clipboard2path-wsl --interval 1000 --output-dir ~/Pictures

# 詳細ログ付きで実行
clipboard2path-wsl --verbose
```

## systemd で自動起動

`init` コマンドで systemd ユーザーサービスも自動的に配置・有効化される:

```bash
clipboard2path-wsl init          # シェルフック + systemd サービス
clipboard2path-wsl init --no-service  # シェルフックのみ
```

手動操作:

```bash
systemctl --user status clipboard2path   # 状態確認
systemctl --user restart clipboard2path  # 再起動
journalctl --user -u clipboard2path -f   # ログ確認
```

SIGTERM/SIGINT でクリーンシャットダウン（ランタイムディレクトリのクリーンアップ）を実行する。

## ビルド

```bash
cargo build --release    # リリースビルド（~670KB）
cargo test               # テスト実行（181 tests）
cargo clippy             # リント
```

## アーキテクチャ

```
src/
  main.rs                  # エントリポイント（DI 組み立て + サブコマンドルーティング）
  domain/                  # 純粋関数（I/O なし）
    image_convert.rs       #   BMP -> PNG 変換
    path_gen.rs            #   保存先パス生成
    wsl_detect.rs          #   WSL2 環境判定
    clipboard_change.rs    #   クリップボード変更検知
    runtime_dir.rs         #   ランタイムディレクトリ解決
    cleanup.rs             #   一時ファイルクリーンアップ
    cli.rs                 #   CLI 引数パース（サブコマンド対応）
    shell_detect.rs        #   シェル検出
    shell_hook.rs          #   シェルフック生成
    path_validate.rs       #   シェル/systemd 埋め込み用パス検証
    systemd_unit.rs        #   systemd unit ファイル生成
    wl_paste_wrapper.rs    #   wl-paste ラッパースクリプト生成
  infra/                   # I/O 層（トレイトで抽象化）
    clipboard.rs           #   wl-paste 呼び出し（読み取り専用）
    command_runner.rs      #   外部コマンド実行の抽象化
    file_system.rs         #   ファイル書き込み
    path_notifier.rs       #   パス通知（latest-path + symlink）
    lifecycle.rs           #   デーモンライフサイクル管理
    shell_installer.rs     #   シェルフック書き込み
    systemd_installer.rs   #   systemd unit 配置・有効化
    wrapper_installer.rs   #   wl-paste ラッパー設置（マーカーベース所有権判定）
  service/                 # オーケストレーション
    converter.rs           #   変換フロー
    daemon.rs              #   ポーリングループ
```

- **ドメイン層**: 全て純粋関数。外部依存ゼロ。
- **インフラ層**: トレイトで抽象化し DI 可能。テスト時はモック差し替え。
- **サービス層**: ドメイン関数の呼び出しのみ。ビジネスロジックなし。

## 設計上の特徴

- **クリップボード非書き換え**: `wl-paste` のみ使用。Windows 側クリップボードに影響しない。
- **ファイル経由のパス通知**: `latest-path` ファイル + `latest.png` シンボリックリンク。
- **アトミック更新**: 一時ファイル -> rename でパス通知もシンボリックリンク更新も安全。
- **シェルフック統合**: Alt+V が画像クリップボード時はパスを、テキスト時は通常ペーストを実行。
- **wl-paste ラッパー**: `image/png` 要求にデーモン保存済み PNG を返し、Claude Code 等の画像ペーストに対応。既存ファイルはマーカーベースの所有権判定で保護。

## ライセンス

[MIT](LICENSE)
