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

This is a selective projection of the Rust model. Metadata, note attributes, EX
directions, AIR heights/colors, and AIR Crush intervals are not exposed. Unsupported
kinds remain distinguishable. XLAIR-mode omission and reporting follow the Rust parser.

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
