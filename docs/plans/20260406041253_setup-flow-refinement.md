# セットアップフロー洗練化

**Cycle ID:** `20260406041253`
**Started:** 2026-04-06 04:12:53
**Status:** 🟢 Complete

---

## 📝 What & Why

`clipboard2path-wsl init` をワンコマンドセットアップに進化させる。現状はシェルフック設置のみで、systemd service の配置・有効化が手動。`uninstall` も対称的に全リソースを除去する。全操作は冪等であること。

## 🎯 Goals

- `init` でシェルフック + systemd service を一括セットアップ
- `uninstall` でシェルフック + systemd service を一括除去
- `status` サブコマンドで現在の状態を確認可能に
- 全操作が冪等（何度実行しても同じ結果、エラーにならない）
- リリースビルドで動作確認

## 📐 Design

### Files to Change

```
src/
  domain/
    mod.rs              - systemd_unit モジュール追加
    systemd_unit.rs     - [新規] unit ファイル生成（純粋関数）
    cli.rs              - InitArgs/UninstallArgs に --no-service 追加、Status コマンド追加
  infra/
    mod.rs              - systemd_installer, command_runner モジュール追加
    command_runner.rs   - [新規] CommandRunner トレイト + RealCommandRunner
    systemd_installer.rs - [新規] SystemdInstaller トレイト + FsSystemdInstaller
    shell_installer.rs  - is_installed() メソッド追加（status 用）
  main.rs              - run_init/run_uninstall 拡張、run_status 追加
```

### Constants

- **Service name**: `clipboard2path.service` — `domain/systemd_unit.rs` に `SERVICE_NAME` 定数として定義。全箇所でこの定数を参照
- **Unit install path**: `~/.config/systemd/user/clipboard2path.service`

### Key Points

- **ドメイン/インフラ分離**: unit ファイル文字列生成はドメイン層（純粋関数）、systemctl コマンド実行はインフラ層
- **CommandRunner トレイト**: `std::process::Command` をトレイトで抽象化。戻り値は `Result<String, String>`（stdout を返す。status サブコマンドで `systemctl --user is-active` の出力が必要なため）
- **冪等性設計**:
  - `init`: unit ファイルは常に上書き → daemon-reload → enable --now（すでに有効でもOK）
  - `uninstall`: stop（未起動でもOK） → disable（未有効でもOK） → ファイル削除（存在しなくてもOK） → daemon-reload
  - シェルフック: init 時は `force=true` で常に最新化
- **ExecStart パス解決**: `std::env::current_exe()` → `canonicalize()` で絶対パス（シンボリックリンク解決済み）を取得
- **部分失敗の扱い**: init でシェルフック成功・systemd 失敗の場合、成功分はそのまま残す（ロールバックしない）。各ステップの結果を個別にログ出力し、最終的に失敗があればエラー終了コードを返す
- **unit ファイルパーミッション**: 0o644 で書き出し（systemd 標準）

### status 出力フォーマット

```
clipboard2path-wsl status:
  service: active (running)      # or: inactive / not installed
  shell hook: installed (fish)   # or: not installed
  latest image: /run/user/1000/clipboard2path/clipboard-20260406-123456.png
```

## 🔧 Implementation Steps

### Phase 1: systemd service インストール機能

**Step 1: `domain/systemd_unit.rs` — unit ファイル生成**
- `SERVICE_NAME` 定数: `"clipboard2path.service"`
- `generate_unit(exec_path: &str, uid: u32) -> String` 純粋関数
- [Unit] Description, After=graphical-session.target
- [Service] Type=simple, ExecStart={exec_path}, Restart=on-failure, RestartSec=5, Environment=WAYLAND_DISPLAY=wayland-0, Environment=XDG_RUNTIME_DIR=/run/user/{uid}
- [Install] WantedBy=default.target
- `unit_install_path(home: &str) -> String` — `{home}/.config/systemd/user/{SERVICE_NAME}` を返す純粋関数
- `domain/mod.rs` にモジュール追加
- 影響ファイル: `src/domain/systemd_unit.rs` (新規), `src/domain/mod.rs`

**Step 2: `infra/command_runner.rs` — コマンド実行抽象化**
- `CommandRunner` トレイト: `fn run(&self, program: &str, args: &[&str]) -> Result<String, String>` — 成功時はstdout、失敗時はエラーメッセージを返す
- `RealCommandRunner`: `std::process::Command` で実行。stdout を String で返す
- テスト用 `MockCommandRunner`: 呼び出し履歴を `Vec<(String, Vec<String>)>` に記録、設定可能なレスポンスを返す
- 影響ファイル: `src/infra/command_runner.rs` (新規), `src/infra/mod.rs`

**Step 3: `infra/systemd_installer.rs` — systemd サービス管理**
- `SystemdInstaller` トレイト: `install(unit_content: &str, home: &str) -> Result<(), InstallError>` / `uninstall(home: &str) -> Result<(), InstallError>` / `is_active(home: &str) -> Result<String, String>` / `is_installed(home: &str) -> bool`
- `FsSystemdInstaller<R: CommandRunner>`: DI で CommandRunner を受け取る
- install: ディレクトリ作成 → unit ファイル書き出し (0o644) → daemon-reload → enable --now（冪等）
- uninstall: stop（失敗無視） → disable（失敗無視） → ファイル削除（存在しなくてもOK） → daemon-reload（冪等）
- is_active: `systemctl --user is-active clipboard2path` の出力を返す（active/inactive/failed）
- is_installed: `unit_install_path()` のファイル存在チェック
- 影響ファイル: `src/infra/systemd_installer.rs` (新規), `src/infra/mod.rs`

**Step 4: CLI 拡張 — `--no-service` フラグと `Status` コマンド**
- `InitArgs` に `no_service: bool` 追加
- `UninstallArgs` に `no_service: bool` 追加
- `Command::Status` バリアント追加
- `parse_args` で `status` サブコマンドパース
- `help_text()` 更新（status コマンドと --no-service オプション追加）
- 影響ファイル: `src/domain/cli.rs`

**Step 5: `main.rs` — init/uninstall 統合**
- `run_init`: シェルフック設置（force=true で冪等） + systemd install（`--no-service` でスキップ）
- `run_uninstall`: シェルフック除去 + systemd uninstall（`--no-service` でスキップ）
- `current_exe()` → `canonicalize()` でバイナリパス取得
- `uid` は `unsafe { libc::getuid() }` で取得（libc 依存追加せず、`/proc/self/status` のUid行をパースする純粋関数で代替）
- 各ステップの成功/スキップを個別にログ出力
- いずれかのステップが失敗した場合、残りは続行しつつ最終的にエラー終了コード
- 影響ファ���ル: `src/main.rs`

### Phase 2: UX 改善

**Step 6: 完了メッセージの改善**
- init 完了時:
  ```
  ✔ shell hook installed (fish)
  ✔ systemd service installed and started
  
  Next steps:
    1. Restart your shell (or run: exec $SHELL)
    2. Verify: systemctl --user status clipboard2path
  ```
- uninstall 完了時:
  ```
  ✔ shell hook removed (fish)
  ✔ systemd service stopped and removed
  ```
- Verbosity 対応（Quiet 時はサマリーを抑制、エラーのみ表示）
- 影響ファイル: `src/main.rs`

**Step 7: `status` サブコマンド実装**
- `ShellInstaller` トレイトに `is_installed(&self, shell: ShellType) -> bool` メソッド追加
- `FsShellInstaller` 実装: fish はファイル存在チェック、bash/zsh は rc ファイル内のマーカー存在チェック
- status 出力: service 状態 + shell hook 状態 + latest-path 内容（上記フォーマット）
- systemd 未インストール時: `service: not installed`
- latest-path 未存在時: `latest image: (none)`
- 影響ファイル: `src/main.rs`, `src/infra/shell_installer.rs`

### Phase 3: リリースビルド & 検証

**Step 8: リリースビルドと動作確認**
- `cargo build --release` でビルド
- バイナリサイズ計測（v2: 668KB からの変化を記録）
- `--help`, `--version`, `status`, `init --no-service` の動作確認
- 不要になった `clipboard2path.service` ファイルをリポジトリから削除（生成に置き換え）
- CLAUDE.md 更新（status サブコマンドの記載追加）
- 影響ファイル: `clipboard2path.service` (削除), `CLAUDE.md`

## ✅ Tests

### domain/systemd_unit.rs
- [ ] generate_unit に正しいバイナリパスとUIDを渡すと、有効な unit ファイル文字列が生成される
- [ ] [Unit], [Service], [Install] の全セクションが含まれる
- [ ] ExecStart にバイナリパスが含まれる
- [ ] Environment に WAYLAND_DISPLAY と XDG_RUNTIME_DIR が含まれる
- [ ] XDG_RUNTIME_DIR に正しい UID が埋め込まれる
- [ ] unit_install_path が正しいパスを返す
- [ ] SERVICE_NAME 定数が "clipboard2path.service" である

### infra/command_runner.rs
- [ ] MockCommandRunner が呼び出し履歴を正しく記録する
- [ ] MockCommandRunner が設定されたレスポンスを返す

### infra/systemd_installer.rs
- [ ] install がディレクトリ作成 → ファイル書き出し → daemon-reload → enable --now の順でコマンド実行する
- [ ] install を2回実行しても成功する（冪等性）
- [ ] uninstall が stop → disable → ファイル削除 → daemon-reload の順で実行する
- [ ] uninstall でサービスが存在しなくてもエラーにならない（冪等性）
- [ ] is_installed がファイル存在を正しく返す
- [ ] is_active が CommandRunner の出力をそのまま返す

### domain/cli.rs
- [ ] `init --no-service` が InitArgs.no_service=true にパースされる
- [ ] `uninstall --no-service` が UninstallArgs.no_service=true にパースされる
- [ ] `status` コマンドが正しくパースされる
- [ ] help_text に status と --no-service が含まれる

### infra/shell_installer.rs
- [ ] is_installed が fish hook ファイルの存在を検出する
- [ ] is_installed が bash/zsh rc のマーカーを検出する
- [ ] is_installed がフック未設置時に false を返す

## 🔒 Security

- [ ] systemd unit ファイルのパーミッション: 0o644（systemd 標準。Step 3 で明示的に設定）
- [ ] ExecStart パスは `current_exe()` → `canonicalize()` でシンボリックリンク解決済みの絶対パス
- [ ] systemctl コマンドの引数に外部入力を含めない（サービス名は定数）
- [ ] HOME パスは `std::env::var("HOME")` 由来（OS提供の信頼できる値）

## 📊 Progress

| Step | Description | Status |
|------|-------------|--------|
| 1 | domain/systemd_unit.rs | 🟢 |
| 2 | infra/command_runner.rs | 🟢 |
| 3 | infra/systemd_installer.rs | 🟢 |
| 4 | CLI 拡張 | 🟢 |
| 5 | main.rs 統合 | 🟢 |
| 6 | 完了メッセージ改善 | 🟢 |
| 7 | status サブコマンド | 🟢 |
| 8 | リリースビルド & 検証 | 🟢 |

**Legend:** ⚪ Pending · 🟡 In Progress · 🟢 Done

---

**Next:** Write tests → Implement → Commit with `claude-skills:commit` 🚀
