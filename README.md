# clipboard2path-wsl

WSL2 上でクリップボードの画像を自動的にファイル保存し、そのパスをクリップボードにセットする軽量デーモン。

## 問題

WSL2 環境ではクリップボードに保存された画像を直接貼り付けられないケースがある。`clipboard2path-wsl` はこの問題を解決する。

## 仕組み

1. クリップボードを監視（ポーリング）
2. 画像（BMP）を検知したら `wl-paste` で取得
3. PNG に変換してファイル保存
4. 保存先パスを `wl-copy` でクリップボードにセット

テキストとしてパスが貼り付け可能になる。

## 必要要件

- WSL2 (WSLg 有効)
- `wl-paste` / `wl-copy` (`wl-clipboard` パッケージ)
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

## 使い方

### デーモンモード（デフォルト）

```bash
clipboard2path-wsl
```

クリップボードを 500ms 間隔で監視し、画像を検知するたびに `/tmp/clipboard-{timestamp}.png` に保存する。

### 単発実行

```bash
clipboard2path-wsl --once
```

クリップボードの画像を 1 回だけ変換して終了する。

### オプション

```
--once              単発実行（デーモンループなし）
--interval <ms>     ポーリング間隔（デフォルト: 500）
--output-dir <path> 出力ディレクトリ（デフォルト: /tmp）
--max-files <n>     保持する最大ファイル数（デフォルト: 100）
--verbose           詳細ログ出力
-q, --quiet         エラー以外の出力を抑制
-h, --help          ヘルプ表示
-v, --version       バージョン表示
```

### 例

```bash
# 1秒間隔で監視、保存先を ~/Pictures に変更
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

## ビルド

```bash
cargo build --release    # リリースビルド（~600KB）
cargo test               # テスト実行（63 tests）
cargo clippy             # リント
```

## アーキテクチャ

```
src/
  main.rs                  # エントリポイント（DI 組み立てのみ）
  domain/                  # 純粋関数（I/O なし）
    image_convert.rs       #   BMP → PNG 変換
    path_gen.rs            #   保存先パス生成
    wsl_detect.rs          #   WSL2 環境判定
    clipboard_change.rs    #   クリップボード変更検知
    debounce.rs            #   自己トリガー防止
    cleanup.rs             #   一時ファイルクリーンアップ
    cli.rs                 #   CLI 引数パース
  infra/                   # I/O 層（トレイトで抽象化）
    clipboard.rs           #   wl-paste / wl-copy 呼び出し
    file_system.rs         #   ファイル書き込み
  service/                 # オーケストレーション
    converter.rs           #   変換フロー
    daemon.rs              #   ポーリングループ
```

- **ドメイン層**: 全て純粋関数。外部依存ゼロ。
- **インフラ層**: トレイトで抽象化し DI 可能。テスト時はモック差し替え。
- **サービス層**: ドメイン関数の呼び出しのみ。ビジネスロジックなし。

## ライセンス

MIT
