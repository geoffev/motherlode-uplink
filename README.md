# Motherlode Price Reporter

Desktop app (Tauri v2) for Motherlode's Datarunners. It watches Star Citizen's
commodity trade terminal, reads prices with local OCR, lets the player check
each reading, and files confirmed ones to [Motherlode](https://cryon.rocks) as
price reports.

It only ever looks at the Star Citizen window, through the operating system's
screen-capture API. It never touches the game process: no injection, memory
reading, process handles or simulated input.

## Install

Windows 10 and 11. Linux isn't supported yet.

1. Download the installer (`…_x64-setup.exe`) from the
   [latest release](https://github.com/geoffev/motherlode-uplink/releases/latest) and run it.
   If Windows SmartScreen warns about an unknown publisher, choose
   **More info → Run anyway**.
2. Install [Tesseract](https://tesseract-ocr.github.io/tessdoc/Installation.html),
   the text reader the app uses. Keep its default folder
   (`C:\Program Files\Tesseract-OCR`) and the app finds it on its own.
3. Create an API key at [cryon.rocks/profile/developer](https://cryon.rocks/profile/developer)
   and paste it into the app.

The app updates itself when a new version is released. Each release lists
SHA-256 checksums for its files. The [wiki](https://github.com/geoffev/motherlode-uplink/wiki)
covers setup and everyday use in more detail.

## Contributing

Bug reports, screenshots of terminals the app reads badly, and pull requests
are all welcome. Please open an issue before starting on anything large.

### Set up

You need Rust (stable), Node 20+, the [Tauri prerequisites](https://tauri.app/start/prerequisites/)
and Tesseract (see [Install](#install)).

```sh
npm install
npm run tauri dev     # run the app, with live reload for the UI
```

Development builds talk to `http://localhost:3000` by default; change the
server under **Settings**. Release builds always use `https://cryon.rocks`.

To work on reading without the game, use **Read a screenshot…** on the empty
review queue. It runs a saved screenshot of a terminal through the same pipeline
as live capture.

### Before opening a pull request

```sh
npm run build                       # type check + frontend build
cd src-tauri && cargo test --locked # Rust tests
```

The same checks run on every pull request (`.github/workflows/check.yml`),
along with `npm audit`, and must pass before merging.

### Ground rules

- **Never touch the game process.** Star Citizen runs Easy Anti-Cheat. No
  memory access, injection, hooks, handles to the game process, or simulated
  keyboard or mouse input. Capture only the `StarCitizen.exe` window, only
  through OS capture APIs.
- **Nothing is sent without the player confirming it.** Motherlode trusts
  reports as they arrive, so the review step is the only check.
- **The API key stays in the Rust core** and the OS keychain. It never reaches
  the web UI or a config file.
- **Don't commit secrets or personal data:** signing keys, API keys, or
  screenshots showing someone's account details. Attach test screenshots to
  an issue instead of committing them.
- **Comments explain why, not what.** Keep the ones a future contributor would
  need (a game quirk, a protocol rule, an OCR workaround) and skip the rest.

### How it fits together

```
src-tauri/src/
  capture.rs   finds the StarCitizen.exe window (process list, no process handle)
  wgc.rs       Windows Graphics Capture of that window
  pipeline.rs  capture loop: thumbnail hash → steady? → full-frame read → queue
  detect.rs    finds the terminal from OCR'd words (IN DEMAND / IN STOCK lists, location)
  parse.rs     OCR text → prices, SCU, container sizes
  ocr.rs       OcrEngine trait; Tesseract run as a child process
  vision.rs    brightness, thresholds, perceptual hashes, crops
  dedupe.rs    duplicate rows (crop hash) and duplicate values (same key + values)
  store.rs     review queue; crops on disk with retention, plus confirmed labels
  outbox.rs    persisted queue of terminal reports: pacing, 429, 401 pause, backoff, 24 h expiry
  api.rs       Motherlode client
  secrets.rs   API key in the OS keychain
  tray.rs      tray icon and menu
src/           React review UI, settings, FAQ
```

Data lives in the app data dir: `readings.json` (queue), `outbox.json`,
`catalog.json` (commodities and terminals, cached for offline use) and
`crops/<date>/<screen>/` (frame, crops and, for confirmed readings, a
`<reading>.json` with the confirmed values: training labels for the OCR model).

### Releasing (maintainers)

1. Set the same version in `src-tauri/tauri.conf.json`, `package.json` and
   `src-tauri/Cargo.toml`.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. The release workflow builds the installers, signs the update, and creates a
   **draft** release with the changes since the previous tag and checksums.
   Check it, then publish it. Installed copies update on their next start.

The workflow needs the `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets.

## Status

Working: sign-in and Datarunners gate, catalog sync, review queue with crops,
frame outlines, low-confidence underline, edit protection, keyboard shortcuts,
Confirm all, persisted outbox with retry, settings, tray, single instance,
autostart, crop retention, signed auto-updates.

Not done yet:

- **Detection is tuned on a handful of screenshots** (1080p and 3440×1440).
  Other resolutions, aspect ratios and UI scales still need checking.
- **Scrolling lists:** each frame is read on its own; rows from a scrolled list
  arrive as a new screen rather than joining the earlier one.
- **Linux:** capture there uses X11 through xcap; the Wayland ScreenCast portal
  still needs writing behind `capture::Backend`. Linux hasn't been built or run yet.
- **Live capture hasn't been tried against the game itself.**
