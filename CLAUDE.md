---
wiki_root: .wiki
---

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

clipboard2path-wsl — WSL2上でクリップボードに保存された画像を貼り付けられない問題を解決する軽量デーモン。

- クリップボードを監視し、画像貼り付け時にWSL2パスへ変換
- Windowsホスト上での貼り付けはバイパス
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

- ドメインロジック（パス変換・クリップボード解析）は純粋関数として実装し、I/Oから分離する
- クリップボード監視・ファイルシステムアクセスなどの外部依存はトレイトで抽象化し、DI可能にする
- WSL固有のパス変換ロジック（`/mnt/c/...` ↔ `C:\...`）は専用モジュールに集約する
