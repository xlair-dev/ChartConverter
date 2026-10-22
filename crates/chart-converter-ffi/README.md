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

Each push to `main` publishes `ghcr.io/xlair-dev/chartconverter-ffi` with `latest`
and `sha-<full commit SHA>` tags. The image contains target-specific archives under
`/archives` and their extracted contents under `/packages/<Rust target>`. Each
archive contains the generated header, static library, CMake package, native link
dependencies reported by `rustc`, and this README. The Windows archive targets
`x86_64-pc-windows-msvc` and contains a `.lib`; the macOS and Linux archives
currently target `x86_64` and contain a `.a`. These archives contain the
release-profile library and must be used only with the matching target. The Linux
archive is built on Ubuntu 22.04 and requires a compatible glibc runtime. The macOS
archive is built with a 13.0 deployment target, matching XLAIR's minimum macOS
version. The Windows archive uses the statically linked MSVC runtime; consumers
must use a compatible runtime configuration.

Pull the latest package image or pin it to a commit:

```sh
docker pull ghcr.io/xlair-dev/chartconverter-ffi:latest
docker pull ghcr.io/xlair-dev/chartconverter-ffi:sha-<full-commit-sha>
```

For digest-level pinning, use the `sha256` digest printed by `docker pull` as
`ghcr.io/xlair-dev/chartconverter-ffi@sha256:<digest>`.

Copy a target package out of the image:

```sh
docker create --name chart-converter-ffi ghcr.io/xlair-dev/chartconverter-ffi:latest
docker cp chart-converter-ffi:/packages/x86_64-pc-windows-msvc ./chart-converter-ffi
docker rm chart-converter-ffi
```

Replace the target directory with `x86_64-unknown-linux-gnu` or
`x86_64-apple-darwin` as needed. Add the extracted package root to
`CMAKE_PREFIX_PATH` and use `find_package(ChartConverterFFI CONFIG REQUIRED)`.
The image also contains the downloadable ZIPs and their `.sha256` sidecars under
`/archives`. Link the `ChartConverter::FFI` target to the C or C++ consumer; the
package carries the native link dependencies reported by Rust.
