# clipboard2path-wsl 実装方針・ロードマップ

- **日付**: 2026-04-06
- **起源**: チーム壁打ち（Challenger / Explorer / Connector / Grounded）

## 背景・目的

WSL2上でクリップボードに保存された画像を貼り付けられない問題を解決する軽量デーモン。

## 環境検証結果

- `wl-paste --type image/bmp` → **動作OK**（WSLg経由のWaylandクリップボードが有効）
- `wl-paste --type image/png` → **非対応**（BMPのみ利用可能）
- → **BMP取得 → Rust側でPNG変換** の方針が確定

## 確定した設計方針

### コアフロー

1. `wl-paste --type image/bmp` でBMPバイナリ取得
2. Rust内で BMP → PNG 変換してファイル保存
3. `wl-copy` で保存パスをクリップボードにセット

### アーキテクチャ

- ドメインロジック（パス変換・画像変換）は純粋関数、I/Oから分離
- クリップボード監視・FSアクセスはトレイトで抽象化しDI可能に
- WSL固有パス変換ロジックは専用モジュール
- wslpath呼び出しを `PathConverter` トレイトに封じ込め

### 技術選定

| 用途 | 選定 |
|------|------|
| 画像取得 | `wl-paste --type image/bmp`（外部コマンド） |
| パス通知 | `wl-copy`（外部コマンド） |
| 画像変換 | `image` crate（BMP → PNG） |
| デーモン化 | systemd user service |

## チーム議論からの重要知見

### リスク・注意点（Challenger）

- クリップボード形式の多様性（DIB/PNG/CF_HDROPなど）
- WSLgとの競合リスク（二重処理）
- **自己トリガー防止が必須**（デーモンが自身の書き込みで再処理しない冪等性保証）
- 環境多様性（Win10/11、WSL2カーネルVer、WSLg有無）でテスト困難

### 代替案・拡張（Explorer）

- 将来: テキストURL変換、逆変換モード、フックAPI
- デーモン管理: systemd user service推奨
- PowerShellフォールバックは優先度低（wl-pasteが動くため）

### 先行事例・crate（Connector）

- win32yank: 最小設計の参考
- `arboard` crate: 画像クリップボード直接扱い（将来的に検討）
- `wl-clipboard-rs`: イベント駆動化の際に検討

### ロードマップ案（Grounded）

| Phase | 内容 | 工数 |
|-------|------|------|
| Phase 1 — MVP | BMP取得→PNG変換→ファイル保存→パス上書き。PNGのみ出力。WSL2判定。テスト付き | 3-4h |
| Phase 2 — 堅牢化 | ポーリング差分検出、エラーハンドリング、systemd化 | 4-6h |
| Phase 3 — 最適化 | イベント駆動、設定ファイル、バイナリサイズ最適化（目標500KB以下） | 3-5h |
