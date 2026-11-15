# Porting scope

Upstream: huai-bot/releasehub 0.2.0, MIT (2026-11-15 snapshot).

| Upstream surface | Plugin result |
| --- | --- |
| Home / library / updates / project detail | Retained |
| release selection and GitHub API safety | Retained, current-platform/architecture recommendation |
| downloads, cancellation, retries, checksum, interruption recovery | Retained Rust background runtime + host native event pump |
| ZIP managed installation, manual records, version switch | Retained, confirmation and isolated directories; Unix executable bits preserved without setuid |
| account device login | Pinned official CLI distributed per platform, isolated config and credential environment |
| GitHub searches / Stars / owned / activity / README / confirmed metadata edit | Retained |
| PAT / optional external callback login | Added, plugin-scoped OS keyring, host-state protection |
| preferences / import-export | Retained excluding standalone tray and all AI settings |
| standalone desktop dialogs / opener / clipboard / theme | Host SDK / official Tauri imports via compatibility shim |
| AI connector / Agent / MCP / legacy build tasks | Removed |
| standalone tray / app packaging / self updates | Host responsibility, removed |

Supported native package targets: macOS arm64/x64, Windows x64/arm64, Linux x64/arm64. DMG/PKG/DEB/RPM installers open only after confirmation. TAR/AppImage can be downloaded but not treated as automatically installed. ZIP preparation is conservative and rejects symlinks, path traversal, duplicate names and unsafe special files.
