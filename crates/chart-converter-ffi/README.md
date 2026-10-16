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

Static consumers must link Rust's native platform dependencies. CMake consumers can
use Corrosion for target selection and linking.
