//! Controller layouts: a picture of a controller and where each control sits on it, for the
//! editor. Profiles say what controls do; layouts only say how to draw them. An Xpadder
//! `.xpaddercontroller` file imports as a layout.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::Button;

/// Something with a place on a controller picture. Sticks are placed by their click buttons
/// (`left_stick`, `right_stick`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Control {
    Button(Button),
    LeftTrigger,
    RightTrigger,
}

impl fmt::Display for Control {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Control::Button(button) => button.name(),
            Control::LeftTrigger => "left_trigger",
            Control::RightTrigger => "right_trigger",
        })
    }
}

impl FromStr for Control {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "left_trigger" => Ok(Control::LeftTrigger),
            "right_trigger" => Ok(Control::RightTrigger),
            _ => Button::from_name(s)
                .map(Control::Button)
                .ok_or_else(|| format!("unknown control `{s}`")),
        }
    }
}

impl Serialize for Control {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Control {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
    }
}

/// A controller picture and where each control sits on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub name: String,
    /// PNG picture, relative to the layout file. Magenta (#FF00FF) pixels are transparent, as in
    /// Xpadder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Picture size in pixels; positions are in these coordinates.
    pub size: [f32; 2],
    /// Centre of each control on the picture.
    #[serde(default)]
    pub positions: BTreeMap<Control, [f32; 2]>,
}

impl Layout {
    /// Checks everything serde cannot: a finite, positive size, finite positions, and an image
    /// path that stays beside the layout file (relative, without `..`). Returns every problem
    /// found, each as a readable sentence.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if !self.size.iter().all(|&v| v.is_finite() && v > 0.0) {
            errors.push(format!("size must be a positive width and height, not {:?}", self.size));
        }
        for (control, position) in &self.positions {
            if !position.iter().all(|v| v.is_finite()) {
                errors.push(format!("positions.{control}: {position:?} is not a position"));
            }
        }
        if let Some(image) = &self.image
            && !is_plain_relative_path(image)
        {
            errors.push(format!(
                "image `{image}` must be a path relative to the layout file, without `..`"
            ));
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

/// Whether `path` is relative and cannot climb out of its folder, read with both Windows and
/// Unix separators so the answer is the same on every OS.
fn is_plain_relative_path(path: &str) -> bool {
    let rooted = path.starts_with(['/', '\\']);
    // A drive (`C:`), a URL scheme or an NTFS stream name.
    let prefixed = path.contains(':');
    !path.trim().is_empty()
        && !rooted
        && !prefixed
        && path.split(['/', '\\']).all(|part| part != "..")
}

#[cfg(test)]
mod tests {
    use serde::de::IntoDeserializer;
    use serde::de::value::{Error, StrDeserializer};

    use super::*;

    #[test]
    fn button_names_match_profile_names() {
        for button in Button::ALL {
            let name: StrDeserializer<'_, Error> = button.name().into_deserializer();
            assert_eq!(Button::deserialize(name), Ok(button));
        }
    }

    #[test]
    fn layouts_are_validated() {
        let mut layout = Layout {
            name: "Pad".into(),
            image: Some("pictures/pad.png".into()),
            size: [512.0, 256.0],
            positions: BTreeMap::from([(Control::LeftTrigger, [4.0, -2.0])]),
        };
        assert_eq!(layout.validate(), Ok(()));
        layout.image = Some("./pad.png".into());
        assert_eq!(layout.validate(), Ok(()));

        for image in [
            "../pad.png",
            "a\\..\\..\\pad.png",
            "/etc/pad.png",
            "\\\\server\\pad.png",
            "C:pad.png",
            "",
        ] {
            layout.image = Some(image.into());
            assert_eq!(layout.validate().map_err(|e| e.len()), Err(1), "{image}");
        }
        layout.image = None;
        layout.size = [0.0, f32::INFINITY];
        layout.positions.insert(Control::Button(Button::South), [f32::NAN, 1.0]);
        let errors = layout.validate().unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[1].starts_with("positions.south"), "{errors:?}");
    }

    #[test]
    fn controls_round_trip_as_strings() {
        for control in Button::ALL
            .into_iter()
            .map(Control::Button)
            .chain([Control::LeftTrigger, Control::RightTrigger])
        {
            assert_eq!(control.to_string().parse::<Control>(), Ok(control));
        }
    }
}
