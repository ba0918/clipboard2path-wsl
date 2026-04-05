---
title: "WSL2 クリップボード連携の技術詳細"
category: wsl-interop
tags: [wsl2, wslg, wayland, wl-paste, wl-copy, clipboard]
source_type: internal
---

# WSL2 クリップボード連携の技術詳細

## WSLg によるクリップボード転送

WSL2 では **WSLg**（Windows Subsystem for Linux GUI）がクリップボードの仲介を行う。WSLg は以下のスタックで動作する：

```
Windows Clipboard
    ↓ (WSLg bridge)
Wayland Compositor (/mnt/wslg)
    ↓ (Wayland protocol)
wl-paste / wl-copy (wl-clipboard)
    ↓
Linux Applications
```

### 環境変数

WSLg が有効な環境では以下の環境変数が自動設定される：

- `WAYLAND_DISPLAY=wayland-0`
- `XDG_RUNTIME_DIR=/run/user/{UID}`

systemd サービスとして動かす場合、これらの環境変数を明示的に設定する必要がある（`clipboard2path.service` の `Environment=` 行）。

## wl-paste / wl-copy の動作

### 利用可能な型の確認

```bash
wl-paste --list-types
```

出力例:
```
image/bmp
text/plain;charset=utf-8
```

このコマンドでクリップボードに現在入っているデータの MIME タイプ一覧を取得できる。clipboard2path-wsl はこれを差分検出に利用し、前回と型リストが変わった場合のみ変換処理を実行する（CPU 負荷の削減）。

### BMP 画像の取得

```bash
wl-paste --type image/bmp > /tmp/test.bmp
```

**重要な発見**: 検証環境（WSL2 + WSLg）では `image/png` は非対応で、`image/bmp` のみ取得可能だった。このため、clipboard2path-wsl は BMP 取得 → Rust 内で PNG 変換という方式を採用している。

### テキストのクリップボードへの書き込み

```bash
echo "/tmp/clipboard-12345.png" | wl-copy
```

clipboard2path-wsl では `Command::new("wl-copy")` で子プロセスを起動し、stdin にパス文字列をパイプで渡す。コマンドインジェクション防止のため、シェル経由ではなく直接 `arg()` で引数を渡す設計。

## WSL2 環境の判定

`/proc/version` の内容に "microsoft" または "WSL" が含まれるかで判定する：

```
Linux version 6.6.87.2-microsoft-standard-WSL2 (root@...) (gcc ...) #1 SMP ...
```

この判定は純粋関数 `wsl_detect::is_wsl2(&str) -> bool` として実装され、大文字小文字を区別しない。

## 自己トリガー防止

clipboard2path-wsl は画像を変換した後、パスを `wl-copy` でクリップボードに書き込む。この書き込みがクリップボード変更イベントとして検知され、再度処理が走ると無限ループになる。

対策: **デバウンス機構**。`wl-copy` 実行後のタイムスタンプを記録し、一定時間（デフォルト 1000ms）以内のクリップボード変更は無視する。

```rust
pub fn should_process_event(
    last_write_timestamp_ms: Option<u64>,
    current_timestamp_ms: u64,
    debounce_ms: u64,
) -> bool
```

## 既知の制限

- WSLg が無効な環境（古い WSL2 カーネル）では `wl-paste` / `wl-copy` が動作しない
- Windows 10 の一部環境では WSLg 自体が利用不可
- 画像以外のクリップボードデータ（ファイルパス、リッチテキスト等）は現在対象外
