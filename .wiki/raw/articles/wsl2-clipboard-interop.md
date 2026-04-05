---
title: "WSL2 クリップボード連携の技術詳細"
category: wsl-interop
tags: [wsl2, wslg, wayland, wl-paste, rdp-cliprdr, shell-hook]
source_type: internal
updated: 2026-04-06
---

# WSL2 クリップボード連携の技術詳細

## クリップボード転送スタック

```
Windows Clipboard
    ↓↑ RDP CLIPRDR (双方向)
WSLg Weston Compositor (/mnt/wslg)
    ↓↑ Wayland protocol
wl-paste (読み取りのみ)          ← clipboard2path-wsl はここだけ使う
```

**重要**: 同期は双方向。`wl-copy` で Wayland 側に書くと Windows 側も上書きされる。clipboard2path-wsl v2 では `wl-copy` を完全に排除し、この問題を回避している。

## WSLg の仕組み

- WSLg 内部で **Weston** (Wayland コンポジター) が動作
- Weston の **RDP バックエンド** (`rdprail-shell`) がクリップボードを仲介
- Windows ホスト側の RDP クライアント相当コンポーネントと CLIPRDR 仮想チャネルで通信
- 仲介プロセスは `/mnt/wslg/` 以下で動作する Weston プロセス

### 環境変数

WSLg 有効環境で自動設定：
- `WAYLAND_DISPLAY=wayland-0`
- `XDG_RUNTIME_DIR=/run/user/{UID}`

## wl-paste の利用（読み取り専用）

### 型リスト取得（差分検出）

```bash
wl-paste --list-types
# 出力例: image/bmp, text/plain;charset=utf-8
```

clipboard2path-wsl はこの MIME タイプ一覧を前回と比較することで変更検知を行う。BMP バイナリ全量を毎回取得するのではなく、型リストの比較で CPU 負荷を削減。

### BMP 画像取得

```bash
wl-paste --type image/bmp
```

**検証結果**: `image/png` は非対応、`image/bmp` のみ取得可能（WSLg 環境依存）。

## パス通知メカニズム（v2）

v1 では `wl-copy` でパスをクリップボードに書き戻していたが、v2 ではファイル経由に変更：

```
$XDG_RUNTIME_DIR/clipboard2path/
├── latest-path              ← 最新画像のフルパス（テキスト）
├── latest.png               ← 最新画像へのシンボリックリンク
├── clipboard-1712345678.png ← 保存された画像
└── ...
```

### アトミック更新

`latest-path` とシンボリックリンクはアトミックに更新される：
1. 一時ファイル（`.latest-path.tmp`）に書き込み（パーミッション `0o600`）
2. `rename()` で本体ファイルに差し替え

シンボリックリンクも同様：一時名で `symlink()` → `rename()` で差し替え。

### デーモンライフサイクル

- **起動**: `$XDG_RUNTIME_DIR/clipboard2path/` を `mkdir -p` + `0o700` で作成
- **停止**: SIGTERM/SIGINT でディレクトリ内全ファイル削除 + `rmdir`
- **最悪ケース**: SIGKILL 等でクリーンアップ不可 → tmpfs なので OS 再起動で消える

## シェルフック

シェルの Ctrl+V をフックし、クリップボード内容に応じて動作を分岐：

```
Ctrl+V → クリップボードに image/bmp がある？
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
```bash
clipboard2path_paste() {
    local latest_path="$XDG_RUNTIME_DIR/clipboard2path/latest-path"
    if [[ -f "$latest_path" ]] && wl-paste --list-types 2>/dev/null | grep -q 'image/bmp'; then
        local path; path="$(cat "$latest_path")"
        READLINE_LINE="${READLINE_LINE:0:$READLINE_POINT}${path}${READLINE_LINE:$READLINE_POINT}"
        READLINE_POINT=$(( READLINE_POINT + ${#path} ))
    else
        local text; text="$(wl-paste -n 2>/dev/null)"
        READLINE_LINE="${READLINE_LINE:0:$READLINE_POINT}${text}${READLINE_LINE:$READLINE_POINT}"
        READLINE_POINT=$(( READLINE_POINT + ${#text} ))
    fi
}
bind -x '"\C-v": clipboard2path_paste'
```

### zsh
```zsh
clipboard2path-paste() {
    local latest_path="$XDG_RUNTIME_DIR/clipboard2path/latest-path"
    if [[ -f "$latest_path" ]] && wl-paste --list-types 2>/dev/null | grep -q 'image/bmp'; then
        LBUFFER+="$(cat "$latest_path")"
    else
        LBUFFER+="$(wl-paste -n 2>/dev/null)"
    fi
}
zle -N clipboard2path-paste
bindkey '^V' clipboard2path-paste
```

## 既知の制限

- WSLg 無効環境では `wl-paste` が動作しない
- Windows 10 の一部環境では WSLg 自体が利用不可
- `image/bmp` のみ対応（`image/png` は WSLg で非対応の環境あり）
- シェルフックはターミナル内でのみ有効（GUIアプリには適用されない）
