# OCR redistribution review

Reviewed 2026-11-01. Scope: the Windows OCR bundle produced by
`prepare-ocr.ps1`, including its English language model. This is a technical
license inventory and implementation review, not a legal opinion or a review
of every dependency in the desktop application.

## Components shipped

| Component | Version | License | Included license file |
|---|---|---|---|
| Tesseract | 5.5.3 | Apache-2.0 | `doc/LICENSE-tesseract.txt` |
| English tessdata_fast model | 4.1.0 | Apache-2.0 | `doc/LICENSE-tesseract.txt` |
| Leptonica | 1.87.0 | BSD-2-Clause | `doc/LICENSE-leptonica.txt` |
| libpng | 1.6.59 | libpng-2.0 | `doc/LICENSE-libpng.txt` |
| zlib | 1.3.2 | Zlib | `doc/LICENSE-zlib.txt` |

The source URLs and archive hashes are recorded in `ocr-sources.json`.
The generated `SOURCE-MANIFEST.json` also records the model URL/hash, native
executable hash and build recipe hash. Installed users receive this manifest,
`NOTICE.txt`, Tesseract's upstream `AUTHORS`, and the license files above.

The Tesseract and model licenses require providing the Apache license and
preserving applicable notices. The pinned upstream roots do not contain a
separate NOTICE file. Leptonica requires reproducing its copyright notice,
conditions and disclaimer for binary distribution; its full license is copied.
The libpng and zlib licenses permit redistribution and require truthful origin
and preservation of applicable source notices. Their full licenses are copied
too. No upstream source files are modified by this recipe. Build options are
described in the installed notice. These licenses do not require publishing
corresponding source with these binaries.

## Why the previous package was replaced

The UB Mannheim/MSYS2 package included GPL/LGPL dependencies such as JBIG-KIT,
gettext and libiconv. Generic source links and license texts copied from current
upstream branches did not establish compliance for those exact binaries.
Those binaries, the old DLL manifest and the unrelated license collection have
been removed from the generated bundle and preparation recipe.

The replacement compiles the four native components above from pinned sources
with static linking. Leptonica enables PNG/zlib only. Tesseract disables
archive/network support, TIFF, training tools, OpenMP, the graphics viewer and
debug fonts. The app saves OCR input as PNG, so other codecs are unnecessary.
No additional packages are fetched by CMake. Source archives and the model are
downloaded only by the preparation scripts and checked against SHA-256 pins.

The executable's import table is checked on every preparation run. Only
`KERNEL32.dll` is allowed; delay imports are rejected. No OCR DLLs or Windows
system DLLs are copied into the installer. Microsoft C/C++ runtime code is
linked statically using Release configuration and remains subject to the
Visual Studio/Build Tools license. Use a validly licensed build environment;
do not redistribute compiler tools or Debug runtimes.
Microsoft STL headers identify their license as Apache-2.0 WITH LLVM-exception.
The exception permits distributing code embedded by compilation without the
license/notice conditions in Apache sections 4(a), 4(b) and 4(d); no STL source
files are copied into the installer.

## Release verification

`npm run prepare:ocr` runs in both CI workflows and before Windows packaging.
It builds the native runtime, verifies imports, replaces the generated resource
directory (removing old DLLs), copies notices from the pinned source versions,
and runs English TSV OCR with an empty model environment and system-only PATH.
It fails on a source/model hash mismatch, unexpected DLL import or failed OCR.

When changing source versions or the build recipe, review the corresponding
licenses, optional dependencies and bundled notices again. Rebuild the installer
and extract it into a fresh directory. Confirm that its OCR executable passes
`check-ocr-imports.ps1`, English recognition works using only the extracted
files, all listed licenses are present, and there are no OCR DLLs left over.

Only `.tools/` and `src-tauri/resources/tesseract/` are generated and ignored.
The scripts, source pins, this review and `ocr-NOTICE.txt` must be committed.
The copied license `.txt` files inside generated resources are ignored, but are
still included by Tauri's resource configuration in the shipped installer.

## Primary references

- [Tesseract license at 5.5.3](https://github.com/tesseract-ocr/tesseract/blob/5.5.3/LICENSE)
- [English model license at 4.1.0](https://github.com/tesseract-ocr/tessdata_fast/blob/4.1.0/LICENSE)
- [Leptonica license at 1.87.0](https://github.com/DanBloomberg/leptonica/blob/1.87.0/leptonica-license.txt)
- [libpng license at 1.6.59](https://github.com/pnggroup/libpng/blob/v1.6.59/LICENSE)
- [zlib license at 1.3.2](https://github.com/madler/zlib/blob/v1.3.2/LICENSE)
- [Microsoft redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files)
- [Microsoft STL license and compiled-code exception](https://github.com/microsoft/STL/blob/main/LICENSE.txt)
- [GNU FAQ: redistributing unchanged binaries](https://www.gnu.org/licenses/gpl-faq.html#UnchangedJustBinary)
