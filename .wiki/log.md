# Wiki Operation Log

## [2026-04-06] init | Wiki initialized

- Created wiki structure at `.wiki/`
- Categories: Architecture, WSL Interop, Clipboard, Rust, Research
- Project: clipboard2path-wsl (WSL2 clipboard image paste daemon)

## [2026-04-06] ingest | 3 articles added

- `why-clipboard2path-wsl.md` (Research) — ツールの必要性、問題背景、設計判断
- `wsl2-clipboard-interop.md` (WSL Interop) — WSLg/Wayland技術詳細、wl-paste動作、自己トリガー防止
- `architecture-overview.md` (Architecture) — 3層アーキテクチャ、DI設計、テスト戦略、バイナリ最適化

## [2026-04-06] compile | 3 wiki articles compiled

- `concepts/why-clipboard2path-wsl.md` — ソースからWiki記事に整形、wikilink追加
- `concepts/wsl2-clipboard-interop.md` — ソースからWiki記事に整形、ポーリングフロー図追加
- `concepts/architecture-overview.md` — ソースからWiki記事に整形、レイヤー図・テスト統計表追加

## [2026-04-06] ingest | v2 architecture refresh — 3 articles rewritten

- `why-clipboard2path-wsl.md` (Research) — v2設計変更の動機（RDP CLIPRDR双方向同期問題）、クリップボード非書き換え方針
- `wsl2-clipboard-interop.md` (WSL Interop) — PathNotifier機構、アトミック更新、デーモンライフサイクル、シェルフック（fish/bash/zsh）
- `architecture-overview.md` (Architecture) — v1→v2差分表、PathNotifier/DaemonLifecycle新設、debounce削除、サブコマンドCLI

## [2026-04-06] compile | v2 wiki articles recompiled

- `concepts/why-clipboard2path-wsl.md` — RDP CLIPRDR双方向同期問題、v2設計方針、ユーザー体験表
- `concepts/wsl2-clipboard-interop.md` — パス通知メカニズム、アトミック更新、デーモンライフサイクル、fish/bash/zshフック
- `concepts/architecture-overview.md` — v1→v2変更サマリー表、PathNotifier/DaemonLifecycleトレイト、90テスト
