# AGENTS.md

This file provides guidance to AI coding agents when working with code in this repository.

## Project Overview

clipboard2path-wsl — WSL2上でクリップボードの画像をファイル保存し、パスをシェルフック経由で提供する軽量デーモン。

- クリップボードを読み取り専用で監視（wl-paste のみ、wl-copy 不使用）
- パス通知はファイル経由（latest-path + latest.png シンボリックリンク）
- シェルフック（fish/bash/zsh）で Alt+V 時にパスを入力
- wl-paste ラッパー（`~/.local/bin/wl-paste`）で Claude Code の画像ペースト（Alt+V → chat:imagePaste）に対応
- Windows側クリップボードに影響しない設計
- 動作の軽さ・バイナリサイズの小ささを最優先

## Tech Stack

- Language: Rust
- Target: WSL2 (Linux) + Windows clipboard interop

## Build / Test / Lint

```bash
cargo build          # ビルド
cargo test           # テスト
cargo clippy         # リント
cargo fmt            # フォーマット
cargo fmt -- --check # フォーマットチェック
```

## Architecture Guidelines

- ドメインロジック（パス変換・クリップボード解析・シェル検出）は純粋関数として実装し、I/Oから分離する
- クリップボード監視・ファイルシステムアクセスなどの外部依存はトレイトで抽象化し、DI可能にする
- クリップボードは読み取り専用（ClipboardReader のみ）。書き込みは PathNotifier 経由でファイルに出力
- サブコマンド: Watch（デフォルト）、Init、Uninstall、Status
- `init` はシェルフック設置 + systemd service 配置・有効化 + wl-paste ラッパー設置をワンコマンドで実行（`--no-service` でサービスとラッパーをスキップ可能）
- `uninstall` はシェルフック除去 + systemd service 停止・削除 + wl-paste ラッパー除去を実行
- `status` はサービス状態、シェルフック状態、wl-paste ラッパー状態、最新画像パスを表示
- init/uninstall/status のオーケストレーションは service 層の `SetupService`（installer トレイト群を DI）が担い、main.rs は引数パース・DI 組み立て・出力のみを行う
- シェル自動検出は `$SHELL` を優先し、`$SHELL` が空・未対応シェルのときのみログインシェル（getent）へフォールバックする
- wl-paste ラッパーはマーカーベースの所有権判定で既存ファイルを保護する（`--force` で上書き可）
- systemd unit ファイルは `init` 時に自動生成（リポジトリ内の静的ファイルは不要）
