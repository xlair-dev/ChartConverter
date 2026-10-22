use chart::{ChartMode, NoteKind};

use crate::{SusError, parse, parse_with_diagnostics, parse_with_mode};

#[test]
fn parses_lowercase_definitions_stateful_directives_and_ids_in_both_passes() {
    let source = r#"#TITLE "MiXeD-日本語"
#COMMENT metadata: not a data record
#REQUEST "ticks_per_beat 960"
#REQUEST "enable_priority true"
#BASEBPM 150
#WAVEOFFSET 0.25
#MEASUREBS 1
#00002:3
#00008:Ab
#TIL00:"0'480:0.5"
#TIL0A:"0'0:2"
#HISPEED 00
#MEASUREHS 00
#ATR0A:"rh:1.5,h:2,pr:3"
#ATTRIBUTE 0A
#0001A:12
#HISPEED 0A
#0003AA:12
#NOATTRIBUTE
#0013BA:22
#NOSPEED
#BPMAb:150
"#;
    for mode in [ChartMode::Normal, ChartMode::Xlair] {
        let expected = parse_with_mode(source, mode).unwrap();
        let lowercase = parse_with_mode(&source.to_ascii_lowercase(), mode).unwrap();
        assert_eq!(lowercase, expected);
        let mixed = source
            .replace("#REQUEST", "#ReQuEsT")
            .replace("ticks_per_beat", "TiCkS_PeR_BeAt")
            .replace("enable_priority true", "EnAbLe_PrIoRiTy TrUe")
            .replace("rh:1.5,h:2,pr:3", "RH:1.5,H:2,Pr:3")
            .replace("#BPMAb", "#bPmAb")
            .replace("#TIL", "#tIl");
        assert_eq!(parse_with_mode(&mixed, mode).unwrap(), expected);
    }
}

#[test]
fn lowercase_directional_requests_keep_document_wide_last_request_precedence() {
    for (last, expected_count) in [("InDePeNdEnT", 2), ("CoNsUmE_TaP", 1)] {
        let source = format!(
            "#request \"XLAIR_DIRECTIONAL_MODE independent\"\n#00010:14\n#00050:34\n#rEqUeSt \"XlAiR_dIrEcTiOnAl_MoDe {last}\"\n"
        );
        let chart = parse_with_mode(&source, ChartMode::Xlair).unwrap();
        assert_eq!(chart.notes().len(), expected_count);
    }
}

#[test]
fn symbolic_standard_air_parents_are_case_insensitive() {
    let uppercase = parse("#00000:010008\n#00000:070008TAP\n").unwrap();
    let lowercase = parse("#00000:010008\n#00000:070008tAp\n").unwrap();
    assert_eq!(lowercase, uppercase);
    assert!(matches!(lowercase.notes()[1].kind(), NoteKind::Air { .. }));
}

#[test]
fn lowercase_default_bpm_directive_and_uppercase_boolean_are_accepted() {
    assert_eq!(
        parse("#bpm_def 150\n").unwrap(),
        parse("#BPM_DEF 150\n").unwrap()
    );
    let chart = parse("#rEqUeSt \"ENABLE_PRIORITY FALSE\"\n").unwrap();
    assert_eq!(chart.priority_enabled(), Some(false));
}

#[test]
fn case_normalization_preserves_error_values_and_source_lines() {
    let report = parse_with_diagnostics(
        "#title \"MiXeD\"\n#request \"TiCkS_PeR_BeAt NotAnInteger\"\n",
        ChartMode::Xlair,
    );
    assert_eq!(
        report.result,
        Err(SusError::InvalidValue {
            line: 2,
            value: "NotAnInteger".to_owned(),
        })
    );
}
