# PowerShell PNG 直接取得 & wl-paste ラッパー

**Cycle ID:** `20260406204808`
**Started:** 2026-04-06 20:48:08
**Status:** 🔵 Implementing

---

## 📝 What & Why

WSLg のクリップボードブリッジは `image/bmp` しか提供しない。Claude Code の `chat:imagePaste` は `image/png` を先に試し、BMP フォールバック時に内蔵 sharp が BI_BITFIELDS 圧縮の BMP→PNG 変換に失敗する。これにより Claude Code で画像ペーストが一切動作しない。

本計画では 2 つのアプローチを組み合わせて解決する：

1. **wl-paste ラッパー**（Phase 1・必須）— デーモンが保存済みの PNG を `wl-paste --type image/png` 経由で返し、Claude Code の sharp 変換をスキップさせる
2. **PowerShell クリップボードリーダー**（Phase 2・オプション）— WSLg を完全バイパスし、Windows .NET API から PNG を直接取得する

Phase 1 だけで Claude Code の画像ペースト問題は解決する。Phase 2 は wl-paste 依存を排除したい場合のオプション。

## 🎯 Goals

- Claude Code の `chat:imagePaste`（Alt+V）で画像を貼り付け可能にする
- Windows 側クリップボードに一切影響を与えない（読み取り専用）
- `wl-copy` を使用しない
- `init` / `uninstall` でワンコマンドセットアップ

## 📐 Design

### Phase 1: wl-paste ラッパー

#### 原理

Claude Code の saveImage コマンドチェーン：
```
xclip -t image/png > file     # 1. 失敗（xclip なし）
wl-paste --type image/png > file  # 2. ★ ラッパーがデーモンの PNG を返す
xclip -t image/bmp > file     # 3. 到達しない
wl-paste --type image/bmp > file  # 4. 到達しない
```

ステップ 2 でラッパーがデーモンの保存済み PNG を返せば、Claude Code は PNG として読み込み（BMP マジックバイト不一致）、sharp 変換をスキップして成功する。

#### Files to Change

```
src/
  domain/
    wl_paste_wrapper.rs  - NEW: ラッパースクリプト生成（純粋関数）+ マーカー定数
    mod.rs               - モジュール追加
    cli.rs               - UPDATE: InitArgs に --force フラグ追加
  infra/
    wrapper_installer.rs - NEW: ラッパーのインストール/アンインストール（マーカーベース所有権判定）
    mod.rs               - モジュール追加
  main.rs                - init/uninstall/status にラッパー管理を追加
CLAUDE.md                - UPDATE: init ワークフローにラッパーを追加、アーキテクチャ説明更新
```

#### ラッパースクリプトの設計

インストール先: `~/.local/bin/wl-paste`（PATH で `/usr/bin` より先）

```bash
#!/bin/bash
# clipboard2path-wsl wl-paste wrapper
# MANAGED BY clipboard2path-wsl — DO NOT EDIT
# Bridges daemon's saved PNG to applications requesting image/png

REAL_WL_PASTE="/usr/bin/wl-paste"
LATEST_PNG="${XDG_RUNTIME_DIR}/clipboard2path/latest.png"

# Bail out immediately if XDG_RUNTIME_DIR is unset (match daemon behavior)
[ -z "$XDG_RUNTIME_DIR" ] && exec "$REAL_WL_PASTE" "$@"

# Bail out if --watch, --primary, --seat, or unknown long options are present
# (only intercept simple single-shot --type image/png requests)
for arg in "$@"; do
    case "$arg" in
        --watch|-w|--primary|-p|--seat|-s) exec "$REAL_WL_PASTE" "$@" ;;
    esac
done

# Detect --type image/png or -t image/png request (space or = separated)
want_png=0
prev=""
for arg in "$@"; do
    case "$arg" in
        --type=image/png|-t=image/png) want_png=1; break ;;
    esac
    if [ "$prev" = "--type" ] || [ "$prev" = "-t" ]; then
        [ "$arg" = "image/png" ] && want_png=1 && break
    fi
    prev="$arg"
done

if [ "$want_png" = "1" ] && [ -L "$LATEST_PNG" ] && [ -f "$LATEST_PNG" ]; then
    cat "$LATEST_PNG"
    exit 0
fi

exec "$REAL_WL_PASTE" "$@"
```

**マネージドマーカー:** スクリプト先頭に `MANAGED BY clipboard2path-wsl` コメントを配置。`is_installed()` はファイル存在 + マーカー内容一致で判定。`install()` はマーカーなしの既存ファイルがある場合エラーにする（`--force` で上書き可）。`uninstall()` はマーカー付きの場合のみ削除。

**latest.png 直接読み取り:** `latest-path` テキストファイル経由の間接参照を廃止し、`latest.png` シンボリックリンクを直接 `cat` する。1段の間接参照を排除し、パス文字列の検証も不要。`-L` でシンボリックリンク存在、`-f` で実体存在を確認。

**透過性の保証：**
- `--watch` / `--primary` / `--seat` がある場合 → 即座に real wl-paste に委譲
- `--type image/png` 以外の全コール → `/usr/bin/wl-paste` にそのまま委譲
- `--list-types` → 変更なし（`image/bmp` のまま。Claude Code の checkImage は `image/bmp` でマッチ済み）
- `XDG_RUNTIME_DIR` 未設定時 → real wl-paste に委譲（本体の `runtime_dir.rs` と同じ「未設定=エラー」方針に統一）
- デーモン未処理時 → real wl-paste にフォールバック（Claude Code は BMP を試みる → sharp 失敗 → 従来通りの挙動）

#### Key Points

- **`domain/wl_paste_wrapper.rs`**: `generate_wrapper()` 純粋関数。`/usr/bin/wl-paste` のパスをパラメータで受け取る。マネージドマーカー定数 `WRAPPER_MARKER` もここで定義
- **`domain/cli.rs`**: `InitArgs` に `force: bool` フィールドを追加。`--force` フラグでラッパーのマーカーなし既存ファイルを上書き可能にする
- **`infra/wrapper_installer.rs`**: `WrapperInstaller` トレイト + `FsWrapperInstaller` 実装。`install()`, `uninstall()`, `is_installed()` メソッド。マーカーベースの所有権判定で既存ファイルを保護。`install()` はインストール先ディレクトリ（`~/.local/bin`）が存在しない場合に `create_dir_all` で作成する
- **`main.rs` の `run_init()`**: シェルフック → systemd → **ラッパー** の順でインストール。完了後に `which wl-paste` の解決先を表示して PATH 優先順位を確認
- **`main.rs` の `run_uninstall()`**: ラッパー除去を追加（マーカー付きの場合のみ削除）
- **`main.rs` の `run_status()`**: ラッパーのインストール状態 + `which wl-paste` の解決先を表示

### Phase 2: PowerShell クリップボードリーダー（オプション）

WSLg の wl-paste を完全バイパスし、Windows の .NET API (`System.Windows.Forms.Clipboard`) から PNG を直接取得する。

**メリット:**
- WSLg の BMP 限定問題を根本回避
- image crate の BMP→PNG 変換も不要になる

**デメリット:**
- PowerShell サブプロセスの管理が複雑（起動コスト、常駐、エラー回復）
- 新たな外部依存（powershell.exe）
- コードベースが大幅に複雑化

**判断:** Phase 1 で Claude Code の問題が解決するため、Phase 2 は実装しない。将来 wl-paste 自体に問題が出た場合に再検討する。

## ✅ Tests

### domain/wl_paste_wrapper.rs
- [ ] `generate_wrapper()` がシバン行を含む
- [ ] `generate_wrapper()` が MANAGED BY マーカーを含む
- [ ] `generate_wrapper()` が REAL_WL_PASTE パスを含む
- [ ] `generate_wrapper()` が `--watch` / `--primary` / `--seat` の即委譲ロジックを含む
- [ ] `generate_wrapper()` が `--type image/png` の検出ロジックを含む
- [ ] `generate_wrapper()` が `latest.png` シンボリックリンクの読み取りロジックを含む
- [ ] `generate_wrapper()` が XDG_RUNTIME_DIR 未設定時の即委譲を含む
- [ ] `generate_wrapper()` が exec フォールバックを含む

### infra/wrapper_installer.rs
- [ ] `install()` がインストール先ディレクトリを自動作成する（`~/.local/bin` が存在しない場合）
- [ ] `install()` がスクリプトを正しいパスに書き込む
- [ ] `install()` が実行権限 (0o755) を設定する
- [ ] `install()` がマネージドマーカー付き既存ファイルを上書きする（冪等性）
- [ ] `install()` がマーカーなし既存ファイルでエラーにする（`force=false` 時）
- [ ] `install()` が `force=true` 時にマーカーなし既存ファイルを上書きする
- [ ] `uninstall()` がマーカー付きファイルを削除する
- [ ] `uninstall()` がマーカーなしファイルを削除しない
- [ ] `uninstall()` が存在しないファイルでエラーにならない
- [ ] `is_installed()` がマーカー内容で判定する

### 統合（main.rs の手動テスト）
- [ ] `init` でラッパーがインストールされる
- [ ] `init` 完了時に `which wl-paste` の解決先が表示される
- [ ] `uninstall` でラッパーが除去される
- [ ] `status` でラッパー状態と解決先が表示される
- [ ] Claude Code で Alt+V が画像を貼り付けられる

### bash スクリプト統合テスト（`tests/wrapper_integration.sh` として実装。REAL_WL_PASTE をモックスクリプトに差し替えてテスト）
- [ ] `wrapper --type image/png` + 有効な latest.png → PNG 出力
- [ ] `wrapper --type image/png` + latest.png 不在 → real wl-paste にフォールバック
- [ ] `wrapper -t image/png` + 有効な latest.png → PNG 出力
- [ ] `wrapper --type=image/png` + 有効な latest.png → PNG 出力（`=` 結合形式）
- [ ] `wrapper -t=image/png` + 有効な latest.png → PNG 出力（`=` 結合形式）
- [ ] `wrapper --type image/bmp` → real wl-paste に委譲
- [ ] `wrapper --list-types` → real wl-paste に委譲
- [ ] `wrapper --watch --type image/png` → real wl-paste に委譲（介入しない）
- [ ] `wrapper --primary --type image/png` → real wl-paste に委譲
- [ ] XDG_RUNTIME_DIR 未設定時 → real wl-paste に委譲

## 🔒 Security

- [ ] ラッパーは `latest.png` シンボリックリンクを直接 cat するだけ。パス文字列の検証不要
- [ ] ラッパーのパーミッション: 0o755（実行可能）
- [ ] `/usr/bin/wl-paste` のフルパスをハードコード（PATH injection 回避）
- [ ] マネージドマーカーで install/uninstall が他のファイルを破壊しないことを保証

## 📋 Migration & Compatibility

- **既存ユーザー:** `clipboard2path-wsl init` を再実行すればラッパーが追加インストールされる。v0.2.1 以前の init はラッパーなし
- **`--no-service` との関係:** ラッパーはデーモンの保存済み PNG に依存。`--no-service` 時はラッパーもスキップする
- **keybindings.json:** Claude Code の Alt+V → `chat:imagePaste` バインドは本ツールの管轄外。ユーザーが手動設定する（`~/.claude/keybindings.json`）
- **Performance:** bash 起動オーバーヘッド 3-6ms/call。デーモン 500ms ポーリングでは影響なし。許容事項として扱う

## 🔮 Future: Upstream Bug Report

根本原因は Claude Code の sharp が WSLg の BI_BITFIELDS BMP を処理できないこと。ラッパーはワークアラウンド。将来的に以下の修正が入ればラッパーは不要になる：
- sharp に BI_BITFIELDS サポート追加（upstream patch）
- Claude Code が BMP 検証を sharp 前に実行（Anthropic change）
- WSLg が PNG も提供（unlikely）

## 📊 Progress

| Step | Description | Status |
|------|-------------|--------|
| 1 | `domain/wl_paste_wrapper.rs` — ラッパースクリプト生成 + マーカー定数 | 🟢 |
| 2 | `domain/cli.rs` — InitArgs に `--force` フラグ追加 | ⚪ |
| 3 | `infra/wrapper_installer.rs` — マーカーベースインストール/アンインストール + ディレクトリ作成 | ⚪ |
| 4 | `main.rs` — init/uninstall/status にラッパー統合 + PATH 表示 | ⚪ |
| 5 | CLAUDE.md 更新 | ⚪ |
| 6 | テスト（ユニット + bash 統合） | ⚪ |
| 7 | 手動検証（Claude Code で Alt+V） | ⚪ |

**Legend:** ⚪ Pending · 🟡 In Progress · 🟢 Done

---

**Next:** Write tests → Implement → Commit with `claude-skills:commit` 🚀
