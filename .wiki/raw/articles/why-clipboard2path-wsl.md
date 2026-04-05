---
title: "なぜ clipboard2path-wsl が必要なのか"
category: research
tags: [wsl2, clipboard, problem-statement, wslg, rdp-cliprdr]
source_type: internal
updated: 2026-04-06
---

# なぜ clipboard2path-wsl が必要なのか

## 問題の背景

WSL2 環境では Windows 側でコピーした画像をWSLターミナルに貼り付けできない。一部環境では Alt+V が動作するが、環境依存が大きく安定しない。

## もう1つの問題: クリップボード双方向同期

WSLg のクリップボード同期は RDP CLIPRDR チャネル経由の **双方向同期**。当初は `wl-copy` でパスをクリップボードに書き戻す設計（v1）を採用したが、これは Windows 側のクリップボードも上書きしてしまう。

→ **スクショを Slack に画像として貼り付けたいのに、パス文字列が貼られてしまう**

この問題が v2 設計変更の動機。

## v2 の解決策: クリップボード非書き換え

clipboard2path-wsl v2 は以下の方針で両方の問題を解決する：

1. **クリップボードは読み取り専用** — `wl-paste` のみ使用、`wl-copy` は一切使わない
2. **画像をファイルとして保存** — `$XDG_RUNTIME_DIR/clipboard2path/` にPNG保存
3. **パスはファイル経由で通知** — `latest-path` ファイル + `latest.png` シンボリックリンク
4. **シェルフックで Ctrl+V を拡張** — クリップボードに画像があるときだけパスを挿入

### ユーザー体験

| 操作 | 結果 |
|------|------|
| スクショ → Slack で Ctrl+V | 画像が貼られる（Windows クリップボード無傷） |
| スクショ → WSLターミナルで Ctrl+V | ファイルパスが入力される |
| テキストコピー → ターミナルで Ctrl+V | テキストが貼られる（通常動作） |

追加操作ゼロ。セットアップは `clipboard2path-wsl init` の1回のみ。

## 設計判断

### なぜクリップボードを書き換えないか

WSLg の RDP CLIPRDR チャネルは双方向同期のため、Wayland 側クリップボードの変更が Windows 側に伝搬する。同期を止める手段はない（WSLg の RDP レイヤーで透過的に処理される）。したがって、Windows 側の画像貼り付けを壊さないためには、クリップボードを書き換えない設計が必須。

### なぜ $XDG_RUNTIME_DIR か

- `/tmp/` は全ユーザー読み書き可能 → セキュリティリスク
- `$XDG_RUNTIME_DIR` (`/run/user/{UID}`) はユーザー専用、tmpfs、セッション終了で消える
- Wayland ソケット自体がここにあり、同種のランタイムデータの標準置き場

### なぜシェルフックか

ターミナルのペースト動作を変更するには、シェルレベルでのフックが最も移植性が高い。fish / bash / zsh それぞれに対応したフックを `clipboard2path-wsl init` で自動生成・インストールする。

### なぜ Rust か

- ~670KB のシングルバイナリ、GC なし、ミリ秒起動
- メモリ安全保証でデーモンの長時間稼働が安心
