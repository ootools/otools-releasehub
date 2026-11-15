# Third-party notices

- **ReleaseHub / 追新** — https://github.com/huai-bot/releasehub, MIT, Copyright (c) 2026 huai-bot. The Svelte interface, Rust release/download/library/GitHub operations and original tests were adapted from upstream 0.2.0. Full license: `LICENSE`.
- **OTools plugin SDK** — https://github.com/ootools/otools-plugin-sdk, MIT. Vendored source/compiled compatibility modules in `vendor/otools-plugin-sdk`; full license in that directory. SDK exports unrelated host capabilities but this product does not expose AI/Agent features.
- **GitHub CLI 2.100.0** — https://github.com/cli/cli, MIT, distributed unchanged from official checksum-pinned release archives. `tools/LICENSE.github-cli.txt` accompanies every native plugin package. No configuration or credentials are bundled.
- Build/package helper scripts were adapted from OTools sibling plugins. Runtime dependencies include Svelte, Lucide, marked, DOMPurify, Tokio, reqwest, rusqlite, zip and keyring under their respective open-source licenses; their lockfiles document exact resolved versions.

GitHub and other product names are trademarks of their respective owners. This port is not affiliated with GitHub or the original ReleaseHub author.
