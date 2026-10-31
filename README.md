# Motherlode Price Reporter

Desktop app (Tauri v2) for Motherlode's Datarunners. It watches Star Citizen's
commodity trade terminal, reads prices with local OCR, lets the player check
each reading, and files confirmed ones to Motherlode as price reports.

## Run it

Prerequisites: Rust (stable), Node 20+, the [Tauri prerequisites](https://tauri.app/start/prerequisites/)
for your OS, and **Tesseract** (the stand-in OCR engine) on PATH or set in Settings:

- Windows: the UB Mannheim installer (`C:\Program Files\Tesseract-OCR\tesseract.exe` is found automatically)
- Fedora: `dnf install tesseract` · Debian/Ubuntu: `apt install tesseract-ocr`

```sh
npm install
npm run tauri dev     # the app
npm test              # Rust unit tests
```

**Read a screenshot…** on the empty queue runs a saved screenshot of a terminal
through the same pipeline as live capture.

## How it fits together

```
src-tauri/src/
  capture.rs   OS screen capture behind a Backend trait (xcap today)
  pipeline.rs  capture loop: thumbnail hash → steady? → full-frame read → queue
  detect.rs    finds the terminal from OCR'd words (IN DEMAND / IN STOCK lists, location)
  parse.rs     OCR text → prices, SCU, container sizes
  ocr.rs       OcrEngine trait; Tesseract CLI stand-in
  vision.rs    brightness, Otsu, dHash, crops
  dedupe.rs    duplicate rows (crop hash) and duplicate values (same key + values)
  store.rs     review queue; crops on disk with retention, plus confirmed labels
  outbox.rs    persisted queue of terminal reports: pacing, 429, 401 pause, backoff, 24 h expiry
  api.rs       Motherlode client (the key never reaches the web UI)
  secrets.rs   API key in the OS keychain
  tray.rs      tray icon and menu
src/           React review UI, settings, FAQ
```

Data lives in the app data dir: `readings.json` (queue), `outbox.json`,
`catalog.json` (commodities and terminals, cached for offline use) and
`crops/<date>/<screen>/` (frame, crops and, for confirmed readings, a
`<reading>.json` with the confirmed values: training labels for the OCR model).

## Status

Working: sign-in and Data Surveyors gate, catalog sync, review queue with crops,
frame outlines, low-confidence underline, edit protection, keyboard shortcuts,
Confirm all, persisted outbox with retry, settings, tray, single instance,
autostart, crop retention.

Not done yet:

- **Detection is tuned on a handful of screenshots** (1080p and 3440×1440).
  Other resolutions, aspect ratios and UI scales still need checking.
- **Scrolling lists:** each frame is read on its own; rows from a scrolled list
  arrive as a new screen rather than joining the earlier one.
- **Capture backends:** Windows uses our own Windows Graphics Capture session
  (`wgc.rs`, no process handle); Linux uses xcap's X11 capture. The Wayland
  ScreenCast portal + PipeWire (`ashpd`) with a persisted restore token still
  needs writing behind `capture::Backend`. Linux hasn't been built or run yet,
  and capture hasn't been tried on the game itself.
- **ONNX OCR:** the custom model plugs in as another `ocr::OcrEngine` via `ort`.
- **Updater:** `tauri.conf.json` has a placeholder `pubkey` and endpoint.
  Generate a key with `npm run tauri signer generate` and set the release URL
  before shipping. Update checks only run in release builds.
