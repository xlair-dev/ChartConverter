/* Keep the checks active when testing release-profile packages. */
#ifdef NDEBUG
#undef NDEBUG
#endif
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "chart_converter.h"

#ifdef __cplusplus
#define CC_STATIC_ASSERT static_assert
#else
#define CC_STATIC_ASSERT _Static_assert
#endif

/* Mirror the original public layouts to catch accidental changes in the header. */
typedef struct V1Position {
    uint64_t numerator;
    uint64_t denominator;
} V1Position;

typedef struct V1Lane {
    CcLaneKind kind;
    uint8_t start;
    uint8_t width;
    CcSideButton button;
} V1Lane;

typedef struct V1Point {
    V1Position position;
    V1Lane lane;
    CcPointKind kind;
} V1Point;

typedef struct V1Note {
    uint32_t id;
    V1Position position;
    V1Lane lane;
    CcNoteKind kind;
    int64_t speed_group;
    uint32_t parent;
    CcAirDirection air_direction;
    const CcPoint *points;
    size_t point_count;
} V1Note;

typedef struct V1ChartView {
    const V1Note *notes;
    size_t note_count;
    const CcTempo *tempos;
    size_t tempo_count;
    const CcScroll *scrolls;
    size_t scroll_count;
} V1ChartView;

CC_STATIC_ASSERT(sizeof(CcPosition) == sizeof(V1Position), "v1 position size");
CC_STATIC_ASSERT(sizeof(CcLane) == sizeof(V1Lane), "v1 lane size");
CC_STATIC_ASSERT(sizeof(CcPoint) == sizeof(V1Point), "v1 point size");
CC_STATIC_ASSERT(sizeof(CcNote) == sizeof(V1Note), "v1 note size");
CC_STATIC_ASSERT(sizeof(CcChartView) == sizeof(V1ChartView), "v1 chart view size");
CC_STATIC_ASSERT(offsetof(CcNote, points) == offsetof(V1Note, points), "v1 path offset");
CC_STATIC_ASSERT(offsetof(CcNote, speed_group) == offsetof(V1Note, speed_group), "v1 group offset");

int main(void) {
    assert(cc_api_version() == CC_API_VERSION);
    assert(cc_result_note_details(NULL, 0) == NULL);
    assert(cc_result_metadata(NULL).main_speed_group == -1);
    assert(cc_result_diagnostics(NULL).count == 0);

    const char *source = "#WAVEOFFSET 0.5\n#BASEBPM 150\n"
                         "#REQUEST \"enable_priority false\"\n#00002:3\n"
                         "#TIL00:\"0'0:1\"\n#HISPEED 00\n#MEASUREHS 00\n#00030A:14\n"
                         "#NOSPEED\n#00134A:24\n";
    CcResult *result = cc_parse(CcFormat_Sus, CcMode_Xlair,
                                (const uint8_t *)source, strlen(source));
    assert(cc_result_error(result) == NULL);
    CcChartView chart = cc_result_chart(result);
    CcChartMetadata metadata = cc_result_metadata(result);
    assert(chart.note_count == 1);
    assert(metadata.audio_offset_seconds.has_value);
    assert(metadata.audio_offset_seconds.value == 0.5);
    assert(metadata.priority_enabled.has_value && !metadata.priority_enabled.value);
    assert(metadata.base_bpm.has_value && metadata.base_bpm.value == 150.0);
    assert(metadata.measure_length_count == 1);
    assert(metadata.measure_lengths[0].length.numerator == 3);
    const CcNoteDetails *details = cc_result_note_details(result, chart.notes[0].id);
    assert(details != NULL && details->point_count == chart.notes[0].point_count);
    assert(details->points[0].speed_group == 0);
    assert(details->points[1].speed_group == -1);
    assert(cc_result_note_details(result, UINT32_MAX) == NULL);
    cc_result_free(result);

    source = "#00010:44\n#00010:xx\n";
    result = cc_parse(CcFormat_Sus, CcMode_Xlair, (const uint8_t *)source, strlen(source));
    assert(cc_result_error(result) != NULL);
    CcDiagnosticsView diagnostics = cc_result_diagnostics(result);
    assert(diagnostics.count == 2);
    assert(diagnostics.diagnostics[0].severity == CcDiagnosticSeverity_Warning);
    assert(diagnostics.diagnostics[0].line == 1);
    assert(diagnostics.diagnostics[1].severity == CcDiagnosticSeverity_Error);
    assert(diagnostics.diagnostics[1].line == 2);
    assert(strcmp(diagnostics.diagnostics[0].format, "SUS") == 0);
    cc_result_free(result);
    cc_result_free(NULL);
    return 0;
}
