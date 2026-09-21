use chart::{AirCrushColor, AirDirection, ChartError, ExDirection};

use crate::C2sError;

pub(super) fn parse_ex_direction(line: usize, value: &str) -> Result<ExDirection, C2sError> {
    match value {
        "UP" => Ok(ExDirection::Up),
        "DW" => Ok(ExDirection::Down),
        "CE" => Ok(ExDirection::Center),
        "RC" | "ALL" => Ok(ExDirection::All),
        "LC" | "VLT" => Ok(ExDirection::Wide),
        "LS" | "L" => Ok(ExDirection::Left),
        "RS" | "R" => Ok(ExDirection::Right),
        "BS" | "IN" | "I" => Ok(ExDirection::Inward),
        _ => Err(C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

pub(super) fn parse_air_direction(line: usize, value: &str) -> Result<AirDirection, C2sError> {
    match value {
        "AIR" => Ok(AirDirection::Up),
        "AUR" => Ok(AirDirection::UpperRight),
        "AUL" => Ok(AirDirection::UpperLeft),
        "ADW" => Ok(AirDirection::Down),
        "ADR" => Ok(AirDirection::LowerRight),
        "ADL" => Ok(AirDirection::LowerLeft),
        _ => Err(C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

pub(super) fn parse_air_color(line: usize, value: &str) -> Result<chart::AirColor, C2sError> {
    match value {
        "DEF" => Ok(chart::AirColor::Normal),
        "GRN" | "PPL" => Ok(chart::AirColor::Inverted),
        _ => Err(C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

pub(super) fn parse_air_crush_color(line: usize, value: &str) -> Result<AirCrushColor, C2sError> {
    match value {
        "DEF" => Ok(AirCrushColor::Normal),
        "NON" => Ok(AirCrushColor::Transparent),
        "RED" => Ok(AirCrushColor::Red),
        "ORN" => Ok(AirCrushColor::Orange),
        "YEL" => Ok(AirCrushColor::Yellow),
        "LIM" => Ok(AirCrushColor::Lime),
        "GRN" => Ok(AirCrushColor::Green),
        "AQA" => Ok(AirCrushColor::Aqua),
        "CYN" => Ok(AirCrushColor::Cyan),
        "DGR" => Ok(AirCrushColor::DarkBlue),
        "BLU" => Ok(AirCrushColor::Blue),
        "VLT" => Ok(AirCrushColor::Violet),
        "PPL" => Ok(AirCrushColor::Purple),
        "PNK" => Ok(AirCrushColor::Pink),
        "GRY" => Ok(AirCrushColor::Gray),
        "BLK" => Ok(AirCrushColor::Black),
        _ => Err(C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        }),
    }
}

pub(super) fn parse_air_height(line: usize, value: &str) -> Result<f64, C2sError> {
    let height = value.parse::<f64>().map_err(|_| C2sError::InvalidValue {
        line,
        value: value.to_owned(),
    })?;
    if !height.is_finite() || height < 0.0 {
        return Err(C2sError::Chart {
            line,
            source: ChartError::InvalidAirHeight,
        });
    }
    Ok(height)
}

pub(super) fn parse_u64(line: usize, value: &str) -> Result<u64, C2sError> {
    value.parse().map_err(|_| C2sError::InvalidValue {
        line,
        value: value.to_owned(),
    })
}

pub(super) fn parse_u32(line: usize, value: &str) -> Result<u32, C2sError> {
    parse_u64(line, value)?
        .try_into()
        .map_err(|_| C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        })
}

pub(super) fn parse_u8(line: usize, value: &str) -> Result<u8, C2sError> {
    parse_u64(line, value)?
        .try_into()
        .map_err(|_| C2sError::InvalidValue {
            line,
            value: value.to_owned(),
        })
}
