# ChartConverter C API

```sh
CHART_CONVERTER_HEADER_DIR=include cargo build -p chart-converter-ffi --release --locked
```

This builds a static library and generates `chart_converter.h` from Rust definitions.
Without `CHART_CONVERTER_HEADER_DIR`, the header is written to Cargo's build-script
output directory. Use the header and library from the same source revision.

Call `cc_parse` with a format, mode, and byte buffer. It uses the existing Rust parser
and UTF-8/BOM-marked UTF-16 decoder. A result is returned on success, invalid arguments,
or a caught parser panic. `cc_result_error` returns a borrowed UTF-8 error or null on
success; `cc_result_chart` returns the borrowed view. Free the result exactly once
with `cc_result_free`; do not mutate or free individual views. Getters and free accept
null. Returned memory remains valid until the result is freed.

The view exposes stable note IDs, kinds, lanes, parent references, paths, tempo, and
scroll changes. Positions are rational absolute beats; tempo and scroll changes are
stably sorted by exact position. Hold paths include start, explicit checkpoints,
and end. Sample timing and game judgement generation belong to the consumer.

## Extended read-only API (revision 2)

`CC_API_VERSION` and `cc_api_version()` are `2`. The layouts of `CcChartView`,
`CcNote`, and `CcPoint` are unchanged; additional information is exposed by getters:

| Getter | Data |
| --- | --- |
| `cc_result_metadata(result)` | Measure-length changes, audio offset, base BPM, priority flag, and source base timeline |
| `cc_result_note_details(result, note_id)` | Note attributes, EX/Flick direction, AIR height/color, AIR Crush color/interval, and path-point details |
| `cc_result_diagnostics(result)` | Invocation-local warnings and the fatal error, including format and source line |

All added strings, arrays, and pointers belong to `CcResult` and have the same lifetime
as the original view. Metadata is returned by value but its measure-length array is
borrowed. The note-details getter takes the stable `CcNote.id`, not a sorted array
index. It returns null for an invalid ID, null result, or parse failure. The other
getters return empty views/absent values on null or failed results, except diagnostics,
which remain available after a parse failure. A diagnostic line of `0` means unknown.
Messages are NUL-terminated UTF-8; embedded NULs are escaped as `\0`.

Optional scalar values use `has_value`: zero offsets, zero priorities, and an explicit
`false` flag are distinct from absence. Measure lengths are rational quarter-note
beats and apply from their measure onward. Positive audio offsets delay the audio
relative to the chart. The base BPM is a scroll reference, not an implicit tempo event.
AIR heights use the Rust IR's normalized height units, not raw format-encoded values.

`CcNoteDetails.points` aligns one-to-one with the legacy `CcNote.points`. Each entry
contains the **resolved** speed group and attributes at that point. A speed group of
`-1` means no group speed, while `0` is a real group. For UGC, unassigned notes use
timeline `0`, and `@USETIL 0` explicitly selects it; `@MAINTIL` is stored independently.
The note-level context of a parsed long note is its start-point context, not its end.
For short notes the point array is empty. Control/visible/invisible kinds remain in
the original `CcPoint`; start/end are identified by path order.

Warnings do not make parsing fail: `cc_result_error` is still null after successful
parsing with omitted elements. Diagnostics include warnings emitted before a fatal
error. The C API does not print diagnostics. Rust callers can use each frontend's
`parse_with_diagnostics`, or the converter's `parse_bytes_with_diagnostics`, to obtain
a `ParseReport` without printing; the original Rust parse functions retain stderr
reporting. Writers retain their existing stderr reporting for lossy conversions.

### IR path context and conversion limits

The Rust `Note::point_metadata` entries are optional overrides: `None` inherits the
note's context; an explicit `PointMetadata { speed_group: None, ... }` disables group
speed at that point. `Chart::point_metadata(note_id, index)` resolves this distinction.
The path invariant is checked when assigning overrides and maintained when appending
points. SUS preserves per-point groups and attributes when writing. UGC preserves
per-point groups but cannot represent SUS attributes; no-group notes/points fall back
to UGC timeline `0` and warn if that changes speed. C2S `SLA` applies to whole notes, so
per-point overrides are reported as information loss rather than silently discarded.
Other format limitations (for example `SPDDEF`/`SPDFLD`) are unchanged.
SUS parsing accepts Ched's `TIL00`, `HISPEED 00`, and `MEASUREHS 00` in both modes.
Although the [SUS v2.7 specification](https://gist.github.com/kb10uy/c171c175ba913dc40a73c6ce69da9859)
lists `01`–`zz`, [Ched exports timeline `00`](https://github.com/paralleltree/Ched/blob/master/Ched/Components/Exporter/SusExporter.cs#L83).
It is a real speed group, not `NOSPEED` / `-1`. Group IDs are IR-local: existing
nonzero SUS IDs keep their mappings, while `00` is assigned an unused group after
reserving all nonzero definitions and references, including later and empty ones.
SUS output uses only `01`–`zz`; charts requiring all 1296 output groups return an error
rather than merging groups. Attribute IDs and empty BPM slots retain their previous rules.

### SUS directional-note compatibility

The default XLAIR-mode overlap rules and existing IR layouts are unchanged. A SUS
document can explicitly select SheetsAnalyzer-compatible ordinary-tap handling:

```sus
#REQUEST "xlair_directional_mode consume_tap"
```

`consume_tap` removes at most one Tap 1 per directional note, and only when beat,
lane start, width, and speed group all match. `independent` keeps ordinary taps and
Flicks as separate notes. Names and values are case-insensitive; the last request
applies to the whole document. Normal mode accepts the request without changing
its general-purpose note interpretation. No request retains the existing behavior.

Neither option disables side XTap pairing: an overlapping XTap and diagonal
directional record still produce the existing XTap-parent / directional-AIR-child
IR representation, in either record order. XLAIR must interpret that pair as a side
XTap and not also emit its parent as a playable main-slider XTap. A non-overlapping
XTap remains independent. SUS output adds `independent` when necessary to prevent
already-resolved ordinary tap/side pairs from being merged on re-reading.

### C / C++ smoke tests

The package script requires CMake plus C and C++ compilers and runs both smoke tests
against the packaged header, library, and imported CMake target before creating the ZIP.
To test an extracted package manually:

```sh
cmake -S crates/chart-converter-ffi/tests -B build/c-api-smoke \
  -DCMAKE_PREFIX_PATH=/absolute/path/to/extracted/package -DCMAKE_BUILD_TYPE=Release
cmake --build build/c-api-smoke --config Release
ctest --test-dir build/c-api-smoke -C Release --output-on-failure
```

Static consumers must link Rust's native platform dependencies. The generated CMake
package carries the native link dependencies reported by Rust; Corrosion remains an
option for consumers that build ChartConverter from source.

Each push to `main` publishes a GitHub Release tagged with `sha-<full commit SHA>`
and marks it as the latest release. Version-numbered releases are not created. Each
release contains target-specific ZIP archives and `.sha256` sidecars. An archive
contains the generated header, static library, CMake package, native link
dependencies reported by `rustc`, and this README. The Windows archive targets
`x86_64-pc-windows-msvc` and contains a `.lib`; the macOS and Linux archives
currently target `x86_64` and contain a `.a`. These archives contain the
release-profile library and must be used only with the matching target. The Linux
archive is built on Ubuntu 22.04 and requires a compatible glibc runtime. The macOS
archive is built with a 13.0 deployment target, matching XLAIR's minimum macOS
version. The Windows archive uses the statically linked MSVC runtime; consumers
must use a compatible runtime configuration.

Download the latest archive or pin it to a commit:

```sh
curl -LO https://github.com/xlair-dev/ChartConverter/releases/latest/download/chart-converter-ffi-x86_64-pc-windows-msvc.zip
curl -LO https://github.com/xlair-dev/ChartConverter/releases/latest/download/chart-converter-ffi-x86_64-pc-windows-msvc.zip.sha256
curl -LO https://github.com/xlair-dev/ChartConverter/releases/download/sha-<full-commit-sha>/chart-converter-ffi-x86_64-pc-windows-msvc.zip
```

The archive can be verified with its `.sha256` sidecar. After extracting it, add
its root directory to `CMAKE_PREFIX_PATH` and use
`find_package(ChartConverterFFI CONFIG REQUIRED)`. Link the
`ChartConverter::FFI` target to the C or C++ consumer; the package carries the
native link dependencies reported by Rust.
