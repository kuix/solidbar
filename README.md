# Solidbar

A menu bar client for [solidtime](https://www.solidtime.io), the open-source time tracker. macOS only, written in Rust ([Tauri 2](https://tauri.app) + [Leptos](https://leptos.dev)), modelled on Harvest's Mac app.

Solidbar is not an official solidtime product.

- **Today first.** The panel opens on today, with a week strip of daily totals above it.
- **Fast entry.** `⌘N`, pick a recent project with `⌘1`–`⌘5` or type to search, `⌥1`–`⌥8` for whole hours, `↩`. New entries continue where the day's last entry ended.
- **±1 hour per row.** `⌘=` / `⌘−` lengthen or shorten an entry; later entries of the day move along so nothing overlaps.
- **One organization at a time.** With several, Solidbar asks which one after signing in; Settings changes it.
- **24- or 12-hour times**, like macOS unless set otherwise in Settings; type `930`, `9:30` or `9:30 pm`.
- **Simple timer.** `S` starts a timer like the selected entry (or stops the running one). The menu bar shows the running time, or today's total.
- **Keyboard and VoiceOver first.** Every action has a shortcut; the UI follows WAI-ARIA patterns and is checked against WCAG 2.2 AA with axe in light, dark and increased-contrast modes.

## Signing in

Solidbar signs in with OAuth 2 (authorization code + PKCE) in your browser.

- **app.solidtime.io:** keep the defaults (the official desktop client ID with the `solidtime://` redirect). This needs Solidbar installed in `/Applications`; if the official solidtime desktop app is installed too, macOS may hand the sign-in to that app instead.
- **Self-hosted:** create a public client with a local redirect and choose "Local address" with the same port:

  ```sh
  php artisan passport:client --public --name=Solidbar --redirect_uri=http://127.0.0.1:47821/callback
  ```

Tokens are stored in the login keychain (debug builds use a private file instead, so rebuilds don't trigger keychain prompts).

## Keyboard shortcuts

| Where | Keys | Action |
|---|---|---|
| Anywhere | `⌥⌘T` | Show or hide Solidbar |
| | `⌥⌘N` | New entry |
| | `⌥⌘S` | Stop the timer, or start one on the last project |
| Panel | `←` `→` / `⌥←` `⌥→` | Previous/next day / week |
| | `T` | Today |
| | `↑` `↓` | Select an entry |
| | `↩` or `E` | Edit |
| | `S` | Start/stop timer |
| | `⌘=` `⌘−` | Add/remove an hour |
| | `⌫` | Delete (asks first) |
| | `⌘N` `⌘R` `⌘,` | New entry, refresh, settings |
| | `⌘O` | Change the organization (in Settings) |
| | `⎋` | Close |
| Entry form | `⌘1`–`⌘5` | Recent project |
| | `⌥1`–`⌥8` | Whole hours |
| | `Space` or type | Open the project list (typing searches) |
| | `↑` `↓` in a time | 15 minutes earlier/later (`⇧` for an hour) |
| | `↩` / `⌘↩` | Save / save and start a timer |

The global shortcuts can be changed in Settings.

## Updates

Solidbar looks for a new release on GitHub when it starts and every six hours, and offers it in the panel ("Restart to update"). Settings › Updates checks right away. Updates are signed; the app only installs archives signed with the project's key.

## Development

Requirements: Rust (pinned in `rust-toolchain.toml`), [trunk](https://trunkrs.dev) 0.21 and the Tauri CLI 2.12 (`cargo binstall trunk@0.21.14 tauri-cli@2.12.1`), Node 24 for the end-to-end tests.

```sh
cargo tauri dev                    # run the app
cargo test --workspace             # unit, property and API tests (wiremock)
cd e2e && npm ci && npx playwright install webkit && npx playwright test
cargo tauri build --target universal-apple-darwin
```

macOS only delivers `solidtime://` links to an app bundle, so with `cargo tauri dev` sign in with "Local address"; otherwise the browser's redirect starts whichever bundle claims the scheme (another Solidbar build, or the official solidtime app).

`e2e/tests/release.spec.ts` runs a release build of the UI under the Content-Security-Policy from `tauri.conf.json`: the dev server doesn't apply the policy, so a mistake there would otherwise only show up as a blank panel in the packaged app.

Layout:

- `crates/shared` — types and pure logic used by both sides: the IPC contract, view models, duration parsing, fuzzy matching.
- `crates/core` — no Tauri: solidtime API client, OAuth, keychain, the time model (stacking, ±1h cascade) and the engine that owns state.
- `crates/ui` — the Leptos UI (compiled to WebAssembly).
- `src-tauri` — the macOS shell: menu bar item, the panel (`tauri-nspanel`), global shortcuts, deep links.
- `e2e` — Playwright tests against the UI with a fake backend, including axe accessibility checks.

## Releasing

1. Bump `version` in `Cargo.toml` (workspace) and `src-tauri/tauri.conf.json`.
2. Push a tag: `git tag v0.2.0 && git push origin v0.2.0`.
3. The Release workflow builds a universal, signed and notarized DMG, the update archive and `latest.json`, and attaches them to a draft release. Publishing the draft makes the update available to everyone.

The workflow needs these repository secrets:

| Secret | What |
|---|---|
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` | Developer ID Application certificate, exported as `.p12` and base64-encoded, and its password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Apple ID, an app-specific password for it, and the team ID, for notarization |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The update signing key (`cargo tauri signer generate`) and its password; the public key is in `tauri.conf.json` |

Losing the update signing key means existing installs can't update any more, so keep a copy somewhere safe.

## License

MIT or Apache-2.0, at your option.
