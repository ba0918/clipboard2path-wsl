---
wiki_root: .wiki
---

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

clipboard2path-wsl — WSL2上でクリップボードの画像をファイル保存し、パスをシェルフック経由で提供する軽量デーモン。

- クリップボードを読み取り専用で監視（wl-paste のみ、wl-copy 不使用）
- パス通知はファイル経由（latest-path + latest.png シンボリックリンク）
- シェルフック（fish/bash/zsh）で Ctrl+V 時にパスを入力
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
- サブコマンド: Watch（デフォルト）、Init、Uninstall
