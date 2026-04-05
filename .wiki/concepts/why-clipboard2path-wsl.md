---
title: "なぜ clipboard2path-wsl が必要なのか"
type: wiki
source_refs:
  - raw/articles/why-clipboard2path-wsl.md
created: 2026-04-06
updated: 2026-04-06
category: research
tags: [wsl2, clipboard, motivation, design-decision, rdp-cliprdr]
related:
  - "[[WSL2 クリップボード連携の技術詳細]]"
  - "[[アーキテクチャ概要]]"
---

# なぜ clipboard2path-wsl が必要なのか

## 概要

WSL2 環境ではクリップボード経由の画像貼り付けに制限がある。clipboard2path-wsl は **クリップボードを一切書き換えずに** 画像をファイル保存し、シェルフック経由でパスを提供する軽量デーモン。

## 2つの問題

### 問題1: ターミナルに画像を貼り付けできない

WSL2 ターミナルではクリップボードの画像データをそのまま貼り付けできない。テキストベースの環境ではファイルパスが必要。

### 問題2: クリップボード双方向同期

WSLg のクリップボード同期は **RDP CLIPRDR チャネル経由の双方向同期**。当初の v1 設計では `wl-copy` でパスをクリップボードに書き戻していたが、これが Windows 側クリップボードも上書きしてしまう。

→ スクショを Slack に画像として貼りたいのに、パス文字列が貼られる

## v2 の解決策

| 方針 | 詳細 |
|------|------|
| クリップボード読み取り専用 | `wl-paste` のみ使用、`wl-copy` は排除 |
| ファイル経由パス通知 | `$XDG_RUNTIME_DIR/clipboard2path/latest-path` |
| シェルフックで Ctrl+V 拡張 | 画像→パス、テキスト→通常ペースト |

### ユーザー体験

| 操作 | 結果 |
|------|------|
| スクショ → **Slack** で Ctrl+V | 画像が貼られる（Windows クリップボード無傷） |
| スクショ → **WSLターミナル** で Ctrl+V | ファイルパスが入力される |
| テキストコピー → ターミナルで Ctrl+V | テキストが貼られる（通常動作） |

追加操作ゼロ。セットアップは `clipboard2path-wsl init` の1回のみ。

## 設計判断

### なぜクリップボードを書き換えないか

WSLg の RDP CLIPRDR は双方向同期で、Wayland 側の変更が Windows 側に伝搬する。同期を止める手段がないため、クリップボードを書き換えない設計が必須。

### なぜ $XDG_RUNTIME_DIR か

- `/tmp/` は全ユーザー読み書き可能 → セキュリティリスク
- `$XDG_RUNTIME_DIR` (`/run/user/{UID}`) はユーザー専用、tmpfs、セッション終了で自動消去
- [[WSL2 クリップボード連携の技術詳細|Wayland ソケット自体がここにある]] — 同種のランタイムデータの標準置き場

### なぜ Rust か

| 観点 | 利点 |
|------|------|
| バイナリサイズ | ~670KB のシングルバイナリ |
| メモリ消費 | GC なし、常駐でも数 MB |
| 起動速度 | ミリ秒単位、systemd 向き |
| 安全性 | メモリ安全保証で長時間稼働が安心 |

## 関連項目

- [[WSL2 クリップボード連携の技術詳細]] — RDP CLIPRDR、wl-paste、シェルフックの技術的な動作
- [[アーキテクチャ概要]] — 3層設計、PathNotifier、DaemonLifecycle
