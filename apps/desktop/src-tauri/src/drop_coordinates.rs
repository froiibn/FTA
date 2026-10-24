//! Normalize the actual backend units before matching a native file drop to a DOM device tile.
use tauri::{LogicalPosition, PhysicalPosition};

pub fn css_position(
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Result<LogicalPosition<f64>, &'static str> {
    if !scale_factor.is_finite()
        || scale_factor <= 0.0
        || !position.x.is_finite()
        || !position.y.is_finite()
    {
        return Err("invalid native drop coordinates");
    }
    // Wry 0.57.1 (Cargo.lock)'s Cocoa drag handler supplies NSDraggingInfo/NSView points, despite Tauri's PhysicalPosition
    // wrapper. Dividing those points again on Retina hits the wrong tile (or no tile). Windows supplies
    // physical client pixels and still needs DPI conversion. Recheck backend units when upgrading Wry.
    let divisor = if cfg!(target_os = "macos") {
        1.0
    } else {
        scale_factor
    };
    Ok(LogicalPosition::new(
        position.x / divisor,
        position.y / divisor,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A Retina drop inside the device must stay there instead of being halved into the toolbar.
    #[test]
    fn native_drop_stays_on_its_css_target_at_hidpi() {
        let position = css_position(PhysicalPosition::new(60.0, 90.0), 2.0).unwrap();
        #[cfg(target_os = "macos")]
        assert_eq!((position.x, position.y), (60.0, 90.0));
        #[cfg(not(target_os = "macos"))]
        assert_eq!((position.x, position.y), (30.0, 45.0));
    }

    // Reject unusable measurements rather than allowing a bad scale to choose a different recipient.
    #[test]
    fn invalid_coordinates_and_scales_are_rejected() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(css_position(PhysicalPosition::new(1.0, 1.0), scale).is_err());
        }
        for position in [
            PhysicalPosition::new(f64::NAN, 1.0),
            PhysicalPosition::new(1.0, f64::INFINITY),
        ] {
            assert!(css_position(position, 2.0).is_err());
        }
    }

    #[test]
    fn standard_density_coordinates_are_unchanged() {
        let position = css_position(PhysicalPosition::new(60.0, 90.0), 1.0).unwrap();
        assert_eq!((position.x, position.y), (60.0, 90.0));
    }
}
