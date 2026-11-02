# OCR redistribution review

Reviewed 2026-11-02. Scope: the Windows OCR bundle produced by
`prepare-ocr.ps1`, including its English language model. This is a technical
license inventory and implementation review, not a legal opinion or a review
of every dependency in the desktop application.

## Components shipped

| Component | Version | License | Included license file |
|---|---|---|---|
| Tesseract | 5.5.4 | Apache-2.0 | `doc/LICENSE-tesseract.txt` |
| English tessdata_fast model | 4.1.0 | Apache-2.0 | `doc/LICENSE-tesseract.txt` |
| Leptonica | 1.87.0 | BSD-2-Clause | `doc/LICENSE-leptonica.txt` |

The source URLs and archive hashes are recorded in `ocr-sources.json`.
The generated `SOURCE-MANIFEST.json` also records the model URL/hash, native
library hashes and build recipe hash. Installed users receive this manifest,
`NOTICE.txt`, Tesseract's upstream `AUTHORS`, and the license files above.

The Tesseract and model licenses require providing the Apache license and
preserving applicable notices. The pinned upstream roots do not contain a
separate NOTICE file. Leptonica requires reproducing its copyright notice,
conditions and disclaimer for binary distribution; its full license is copied.
No upstream source files are modified by this recipe. Build options are
described in the installed notice. These licenses do not require publishing
corresponding source with these binaries.

## Why the previous package was replaced

The UB Mannheim/MSYS2 package included GPL/LGPL dependencies such as JBIG-KIT,
gettext and libiconv. Generic source links and license texts copied from current
upstream branches did not establish compliance for those exact binaries.
Those binaries, the old DLL manifest and the unrelated license collection have
been removed from the generated bundle and preparation recipe.

The replacement compiles Tesseract and Leptonica from pinned sources with static
linking. Leptonica disables optional image codecs, including PNG/zlib. Tesseract
disables archive/network support, TIFF, training tools, OpenMP, the graphics viewer,
debug fonts and the unused legacy OCR engine. The app passes grayscale pixels
directly to the native API.
No additional packages are fetched by CMake. Source archives and the model are
downloaded only by the preparation scripts and checked against SHA-256 pins.

Both OCR libraries are statically linked into the Rust app. No OCR executable
or OCR DLL is included in the installer. The OCR Microsoft C/C++ runtime is
linked statically in Release configuration. The old standalone OCR executable
import check does not apply to the desktop application. Use a validly
licensed Visual Studio/Build Tools environment; do not redistribute compiler tools
or Debug runtimes.
Microsoft STL headers identify their license as Apache-2.0 WITH LLVM-exception.
The exception permits distributing code embedded by compilation without the
license/notice conditions in Apache sections 4(a), 4(b) and 4(d); no STL source
files are copied into the installer.

## Release verification

`npm run prepare:ocr` runs in both CI workflows and before Windows packaging.
It builds native static libraries, replaces the generated resource directory
(removing old executables and DLLs), copies notices from pinned source versions,
and records library hashes. The Rust test suite verifies English recognition
through the linked API, word geometry/confidence, and model loading from Windows
verbatim resource paths. OCR accepts no executable path and never spawns a process.

When changing source versions or the build recipe, review licenses, optional
dependencies and notices again. Rebuild the installer and extract it into a fresh
directory. Confirm recognition works, all listed licenses are present, and no
Tesseract executable or OCR DLL remains in the resources.

Only `.tools/` and `src-tauri/resources/tesseract/` are generated and ignored.
The scripts, source pins, this review and `ocr-NOTICE.txt` must be committed.
The copied license `.txt` files inside generated resources are ignored, but are
still included by Tauri's resource configuration in the shipped installer.

## Primary references

- [Tesseract license at 5.5.4](https://github.com/tesseract-ocr/tesseract/blob/5.5.4/LICENSE)
- [English model license at 4.1.0](https://github.com/tesseract-ocr/tessdata_fast/blob/4.1.0/LICENSE)
- [Leptonica license at 1.87.0](https://github.com/DanBloomberg/leptonica/blob/1.87.0/leptonica-license.txt)
- [Microsoft redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files)
- [Microsoft STL license and compiled-code exception](https://github.com/microsoft/STL/blob/main/LICENSE.txt)
- [GNU FAQ: redistributing unchanged binaries](https://www.gnu.org/licenses/gpl-faq.html#UnchangedJustBinary)

## Build helpers still required

`build-ocr.ps1` compiles the two native libraries from pinned sources.
`prepare-ocr.ps1` stages the hash-verified model, exact source licenses and manifest.
`ocr-common.ps1` provides SHA-256 hashing. `ocr-sources.json` records source pins.
The CLI import checker was removed because no OCR executable is distributed.
Neither libpng nor zlib is built or linked by this native OCR recipe.
