use chart::AirDirection;

pub(super) fn is_xlair_side_air_direction(direction: AirDirection) -> bool {
    matches!(
        direction,
        AirDirection::UpperLeft
            | AirDirection::UpperRight
            | AirDirection::LowerLeft
            | AirDirection::LowerRight
    )
}
