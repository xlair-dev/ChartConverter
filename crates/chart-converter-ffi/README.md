# ChartConverter C API

Build the native library and generate its matching C/C++ header:

```sh
CHART_CONVERTER_HEADER_DIR=include cargo build -p chart-converter-ffi --release --locked
```

The crate builds a static library and a shared library. The header is generated from
the Rust API by cbindgen; do not maintain a separate handwritten declaration.
Without `CHART_CONVERTER_HEADER_DIR`, it is written to Cargo's build-script output directory.

```c
#include "chart_converter.h"

void read_chart(const uint8_t *bytes, size_t length) {
    CcResult *result = cc_parse(CcFormat_Sus, CcMode_Xlair, bytes, length);
    const char *error = cc_result_error(result);
    if (!error) {
        CcChartView chart = cc_result_chart(result);
        /* Read chart.notes, chart.tempos, and chart.scrolls here. */
        (void)chart;
    }
    cc_result_free(result);
}
```

## Contract

- `cc_parse` accepts SUS, C2S, and UGC in Normal or XLAIR mode, using the Rust
  `parse_bytes_with_mode` implementation and its UTF-8/BOM-marked UTF-16 decoding.
- Every call returns an owned result, including invalid arguments and caught Rust panics.
  `cc_result_error` is null on success. Errors are NUL-terminated UTF-8 strings.
- Result getters return immutable borrowed views. All arrays, nested paths, and error
  strings remain valid until `cc_result_free`. Free the result exactly once; do not free
  individual views or mutate them. Null is accepted by the getters and free function.
- Positions are reduced rational absolute beat positions. Note IDs are stable insertion
  indices. Parent and note-scoped speed references use those same IDs.
- Tempo and scroll changes are stably sorted by exact rational position before export;
  changes at the same position retain their source order.
- Holds expose start, explicit checkpoints, and end as path points. Slides expose their
  original visible, invisible, and control points. Sample timing and game judgement
  generation are the consumer's responsibility.
- This API projects note kinds, lanes, parent relations, paths, tempo, scroll scopes,
  audio offset, and base BPM. It does not expose format metadata, note attributes,
  EX directions, AIR heights/colors, or AIR Crush intervals and is not a lossless
  replacement for the Rust chart model. Unsupported kinds remain distinguishable;
  consumers must decide whether to accept them.
- XLAIR mode has the same omission/reporting behavior as the Rust parser. It does not
  turn unsupported notation reported by that parser into a C API error.

Static consumers must link the platform libraries required by Rust's standard library.
CMake consumers can use Corrosion to handle these dependencies and target selection.
The C ABI is versioned with this crate's source revision; compile with its generated
header and library from the same revision.
