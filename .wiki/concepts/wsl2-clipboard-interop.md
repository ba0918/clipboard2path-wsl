---
title: "WSL2 クリップボード連携の技術詳細"
type: wiki
source_refs:
  - raw/articles/wsl2-clipboard-interop.md
created: 2026-04-06
updated: 2026-04-06
category: wsl-interop
tags: [wsl2, wslg, wayland, wl-paste, wl-copy, debounce]
related:
  - "[[なぜ clipboard2path-wsl が必要なのか]]"
  - "[[アーキテクチャ概要]]"
---

# WSL2 クリップボード連携の技術詳細

## 概要

WSL2 では WSLg（Windows Subsystem for Linux GUI）がクリップボードの Windows ↔ Linux 間転送を仲介する。clipboard2path-wsl は `wl-paste` / `wl-copy` コマンドを通じてこの仕組みを利用する。

## クリップボード転送スタック

```
Windows Clipboard
    ↓  WSLg bridge
Wayland Compositor (/mnt/wslg)
    ↓  Wayland protocol
wl-paste / wl-copy
    ↓
clipboard2path-wsl
```

### 必要な環境変数

WSLg が有効な環境では以下が自動設定される：

- `WAYLAND_DISPLAY=wayland-0`
- `XDG_RUNTIME_DIR=/run/user/{UID}`

systemd サービスでは `Environment=` で明示的に設定が必要。

## wl-paste / wl-copy の利用

### 型リスト取得（差分検出用）

```bash
wl-paste --list-types
# 出力例: image/bmp, text/plain;charset=utf-8
```

clipboard2path-wsl はこのコマンドで MIME タイプ一覧を取得し、前回と比較することで変更検知を行う。フルデータ（BMP バイナリ）を毎回取得するのではなく、型リストの比較で済ませることで CPU 負荷を削減している。

### BMP 画像取得

```bash
wl-paste --type image/bmp
```

**検証結果**: `image/png` は非対応、`image/bmp` のみ取得可能（WSLg 環境依存）。このため clipboard2path-wsl は BMP 取得 → Rust 内 PNG 変換の方式を採用。

### テキスト書き込み

```rust
// コマンドインジェクション防止: シェル経由ではなく直接引数渡し
Command::new("wl-copy")
    .stdin(Stdio::piped())
    .spawn()
// stdin にパス文字列をパイプ
```

## WSL2 環境判定

`/proc/version` の内容で判定する：

```rust
pub fn is_wsl2(proc_version: &str) -> bool {
    let lower = proc_version.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl")
}
```

典型的な WSL2 の `/proc/version`:
```
Linux version 6.6.87.2-microsoft-standard-WSL2 (root@...) ...
```

## 自己トリガー防止（デバウンス）

### 問題

画像変換後にパスを `wl-copy` で書き込むと、それ自体がクリップボード変更として検知される。放置すると無限ループが発生。

### 対策

タイムスタンプベースのデバウンス機構：

```rust
pub fn should_process_event(
    last_write_timestamp_ms: Option<u64>,
    current_timestamp_ms: u64,
    debounce_ms: u64,     // デフォルト: 1000ms
) -> bool
```

- `wl-copy` 実行時のタイムスタンプを記録
- 記録から `debounce_ms` 以内の変更は無視
- 時計の巻き戻しにも対応（安全側に倒す）

### ポーリングフロー

```
デバウンスチェック → 型リスト取得 → 変更検知 → BMP有無確認 → 変換実行
```

各段階で条件を満たさない場合は早期リターンし、不要な処理を回避。

## 既知の制限

- WSLg 無効環境では `wl-paste` / `wl-copy` が動作しない
- Windows 10 の一部環境では WSLg 自体が利用不可
- 画像以外のクリップボードデータ（ファイルパス、リッチテキスト等）は対象外

## 関連項目

- [[なぜ clipboard2path-wsl が必要なのか]] — このツールの存在意義
- [[アーキテクチャ概要]] — トレイト抽象化による wl-paste/wl-copy の分離
