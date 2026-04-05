---
title: "WSL2 クリップボード連携の技術詳細"
type: wiki
source_refs:
  - raw/articles/wsl2-clipboard-interop.md
created: 2026-04-06
updated: 2026-04-06
category: wsl-interop
tags: [wsl2, wslg, wayland, wl-paste, rdp-cliprdr, shell-hook, atomic-write]
related:
  - "[[なぜ clipboard2path-wsl が必要なのか]]"
  - "[[アーキテクチャ概要]]"
---

# WSL2 クリップボード連携の技術詳細

## 概要

WSLg は RDP CLIPRDR チャネルによる双方向クリップボード同期を提供する。clipboard2path-wsl v2 は `wl-paste` による読み取りのみを使用し、パス通知はファイル経由 + シェルフックで実現する。

## クリップボード転送スタック

```
Windows Clipboard
    ↓↑  RDP CLIPRDR（双方向同期）
WSLg Weston Compositor (/mnt/wslg)
    ↓↑  Wayland protocol
wl-paste（読み取りのみ）  ← clipboard2path-wsl はここだけ使う
```

WSLg 内部の Weston が RDP バックエンド (`rdprail-shell`) を通じて Windows ホストと通信。`wl-copy` による書き込みは Windows 側に伝搬するため、v2 では完全に排除。

## wl-paste の利用

### 型リスト取得（差分検出）

```bash
wl-paste --list-types
```

MIME タイプ一覧を前回と比較し、変更時のみ BMP 全量取得。CPU 負荷の削減。

### BMP 画像取得

```bash
wl-paste --type image/bmp
```

**検証結果**: `image/png` は非対応、`image/bmp` のみ取得可能。Rust 内で PNG に変換。

## パス通知メカニズム（v2）

```
$XDG_RUNTIME_DIR/clipboard2path/
├── latest-path              ← 最新画像のフルパス（テキスト）
├── latest.png               ← 最新画像へのシンボリックリン��
├── clipboard-1712345678.png
└── ...（最大20件、古い順にローテート）
```

### アトミック更新

`latest-path` とシンボリックリンクはアトミックに更新：

1. 一時ファイル（`.latest-path.tmp`）に書き込み（パーミッション `0o600`）
2. `rename()` で本体に差し替え

シンボリックリンクも同様：一時名で `symlink()` → `rename()`。中途半端な状態を防止。

### デーモンライフサイクル

| イベント | 動作 |
|---------|------|
| 起動 | `mkdir -p $XDG_RUNTIME_DIR/clipboard2path/` (`0o700`) |
| 停止（SIGTERM/SIGINT） | ディレクトリ内全ファイル削除 + `rmdir` |
| 異常終了（SIGKILL等） | tmpfs なので OS 再起動で消える |

## シェルフック

`clipboard2path-wsl init` で自動インストール。Ctrl+V をフックし内容に応じて分岐：

```
Ctrl+V
  └→ クリップボードに image/bmp がある？
       ├─ YES → latest-path を読んでパスを入力
       └─ NO  → wl-paste -n で通常テキストペースト
```

### fish

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

### bash

`READLINE_LINE` / `READLINE_POINT` を操作し、`bind -x '"\C-v": clipboard2path_paste'` でバインド。

### zsh

`zle` ウィジェットとして登録し、`bindkey '^V'` でバインド。`LBUFFER` にパスを追加。

## WSL2 環境判定

```rust
pub fn is_wsl2(proc_version: &str) -> bool {
    let lower = proc_version.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl")
}
```

`/proc/version` の文字列で判定。大文字小文字を区別しない。

## 既知の制限

- WSLg 無効環境では `wl-paste` が動作しない
- `image/bmp` のみ対応（`image/png` は WSLg で非対応の環境あり）
- シェルフックはターミナル内でのみ有効（GUI アプリには適用されない）
- Windows Terminal の Ctrl+V がシェルに渡されない場合、ターミナル側の設定変更が必要な場合がある

## 関連項目

- [[なぜ clipboard2path-wsl が必要なのか]] — RDP CLIPRDR 双方向同期が v2 設計変更の動機
- [[アーキテクチャ概要]] — PathNotifier、DaemonLifecycle のトレイト設計
