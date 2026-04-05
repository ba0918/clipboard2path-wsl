# clipboard2path-wsl

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
5. シェルの Ctrl+V フックが `latest-path` を読み取りパスを入力

## 必要要件

- WSL2 (WSLg 有効)
- `wl-paste` (`wl-clipboard` パッケージ)
- Rust toolchain (ビルド時のみ)

```bash
# Ubuntu/Debian
sudo apt install wl-clipboard
```

## インストール

```bash
git clone https://github.com/your-user/clipboard2path-wsl.git
cd clipboard2path-wsl
cargo install --path .
```

## セットアップ

### 1. シェルフックをインストール

```bash
# 自動検出（$SHELL から判定）
clipboard2path-wsl init

# シェルを明示指定
clipboard2path-wsl init fish
clipboard2path-wsl init bash
clipboard2path-wsl init zsh
```

### 2. デーモンを起動

```bash
clipboard2path-wsl
```

以降、クリップボードに画像があるときに Ctrl+V を押すとファイルパスが入力される。
テキストがクリップボードにあるときは通常のペースト動作。

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

### シェルフック管理

```bash
clipboard2path-wsl init [fish|bash|zsh]       # インストール
clipboard2path-wsl init --force [fish|bash|zsh] # 強制上書き
clipboard2path-wsl uninstall [fish|bash|zsh]   # アンインストール
```

### オプション

```
COMMANDS:
    (default)           クリップボード監視（デーモンモード）
    init [SHELL]        シェルフックをインストール
    uninstall [SHELL]   シェルフックをアンインストール

WATCH OPTIONS:
    --once              単発実行（デーモンループなし）
    --interval <ms>     ポーリング間隔（デフォルト: 500）
    --output-dir <path> 出力ディレクトリ（デフォルト: $XDG_RUNTIME_DIR/clipboard2path/）
    --max-files <n>     保持する最大ファイル数（デフォルト: 20）
    --verbose           詳細ログ出力
    -q, --quiet         エラー以外の出力を抑制

INIT OPTIONS:
    -f, --force         既存フックを強制上書き

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

```bash
# サービスファイルをコピー
mkdir -p ~/.config/systemd/user
cp clipboard2path.service ~/.config/systemd/user/

# 有効化・起動
systemctl --user enable clipboard2path-wsl
systemctl --user start clipboard2path-wsl

# ログ確認
journalctl --user -u clipboard2path-wsl -f
```

SIGTERM/SIGINT でクリーンシャットダウン（ランタイムディレクトリのクリーンアップ）を実行する。

## ビルド

```bash
cargo build --release    # リリースビルド（~670KB）
cargo test               # テスト実行（90 tests）
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
  infra/                   # I/O 層（トレイトで抽象化）
    clipboard.rs           #   wl-paste 呼び出し（読み取り専用）
    file_system.rs         #   ファイル書き込み
    path_notifier.rs       #   パス通知（latest-path + symlink）
    lifecycle.rs           #   デーモンライフサイクル管理
    shell_installer.rs     #   シェルフック書き込み
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
- **シェルフック統合**: Ctrl+V が画像クリップボード時はパスを、テキスト時は通常ペーストを実行。

## ライセンス

MIT
