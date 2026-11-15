# v0.1.0 verification

## Published artifacts

- Plugin release: https://github.com/ootools/otools-releasehub/releases/tag/plugin-otools-releasehub-v0.1.0
- Verified build: https://github.com/ootools/otools-releasehub/actions/runs/43690406543 (source `aef39f94af9fd4f9de49f095573697aae3e634ee`)
- Release job: https://github.com/ootools/otools-releasehub/actions/runs/43691164298
- Plugin market returned explicit success on 2026-11-15.
- Every Windows/macOS/Linux x64/arm64 native build passed tests, native ABI smoke, official GitHub CLI checksum preparation and artifact verification. Package job passed all six .oplg checks. Release job revalidated all six before upload and tagged the tested source commit.

## Local evidence

- 60 Rust tests passed (53 unit + 7 integration).
- 54 Svelte/TypeScript tests passed; svelte-check 0 errors/0 warnings.
- Built macOS native ABI exercised via ctypes in an isolated temporary directory: health, SQLite import/snapshot, events and shutdown, without account credentials.
- Two package tests passed, including actual .oplg licensing, CLI checksums/executable mode, production manifest and absence of runtime data.
- Host cargo check -p OTools --lib --locked passed (existing unrelated warnings only); host vue-tsc passed.
- 13 host parser/private Rust sink/mailbox tests, 27 focused SDK tests and 3 Tools routing/native initialization tests passed.

## Integration and review

OTools `main` commits `2cb6f131` and `b1d54024` add generic private parameter routing and single-use source/document-bound authorization callbacks. SDK `main` commit `bc27217` exports the reusable API. xyito registers this plugin under `otools/otools-releasehub`.

Security review fixed upstream raw OS URL broadcasts using a pinned Rust-private deep-link sink, transactional token persistence rollback, concurrent native requests not holding global runtime locks over network calls, and executable archive handling without setuid. No runtime tokens, accounts or SQLite data are committed or packaged.

## Not claimed as tested

No connected browser was available for screenshots/visual QA; mounted real Svelte components, navigation and full-app error tests were used instead. No real account browser authorization, external OAuth service or software installer was executed. The external OAuth service is optional/user-provided; anonymous usage, bundled CLI device authorization and PAT do not require it. Authorization state is process-local and must be restarted after host exit.

No new OTools host installer release was triggered: existing 0.9.4 binaries need a newer host build to expose the optional callback API. Normal plugin/device/PAT operations use existing host APIs.
