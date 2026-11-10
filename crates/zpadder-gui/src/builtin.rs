//! Controller pictures and starter profiles compiled into ZPadder, so they are always there.
//!
//! The pictures are cut out of their background and scaled to 1000 px wide;
//! `assets/controllers/CREDITS.md` lists their sources and licences.

use crate::pad_view::Family;

/// A built-in controller picture: its id (`builtin:<id>` in a profile's `layout`), display name,
/// layout file and PNG.
pub(crate) struct BuiltinPicture {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) layout: &'static str,
    pub(crate) png: &'static [u8],
}

macro_rules! picture {
    ($id:literal, $name:literal, $file:literal) => {
        BuiltinPicture {
            id: $id,
            name: $name,
            layout: include_str!(concat!("../../../assets/controllers/", $file, ".layout.toml")),
            png: include_bytes!(concat!("../../../assets/controllers/", $file, ".png")),
        }
    };
}

pub(crate) const PICTURES: [BuiltinPicture; 4] = [
    picture!("dualshock4", "DualShock 4 (PS4)", "dualshock4"),
    picture!("xbox-one", "Xbox One / Series", "xbox-one"),
    picture!("xbox-360", "Xbox 360", "xbox-360"),
    picture!("switch-pro", "Switch Pro Controller", "switch-pro"),
];

pub(crate) const PREFIX: &str = "builtin:";

/// The built-in picture a `layout` value names, if it names one.
pub(crate) fn picture(layout: &str) -> Option<&'static BuiltinPicture> {
    let id = layout.strip_prefix(PREFIX)?;
    PICTURES.iter().find(|picture| picture.id == id)
}

/// The photo that best matches a controller family.
pub(crate) fn picture_for(family: Family) -> &'static BuiltinPicture {
    let id = match family {
        Family::PlayStation => "dualshock4",
        Family::Xbox => "xbox-one",
        Family::Nintendo => "switch-pro",
    };
    PICTURES.iter().find(|picture| picture.id == id).expect("every family has a picture")
}

/// A starter profile: display name, short description and TOML.
pub(crate) struct Template {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) toml: &'static str,
}

pub(crate) const TEMPLATES: [Template; 5] = [
    Template {
        name: "Desktop & browsing",
        description: "Left stick moves the pointer, right stick scrolls, face buttons click.",
        toml: include_str!("../../../profiles/templates/desktop.toml"),
    },
    Template {
        name: "First-person shooter",
        description: "WASD on the left stick, mouse look on the right, triggers fire and aim.",
        toml: include_str!("../../../profiles/templates/fps.toml"),
    },
    Template {
        name: "Platformer / arcade",
        description: "Arrow keys on the stick and D-pad, Z/X/A/S on the face buttons.",
        toml: include_str!("../../../profiles/templates/platformer.toml"),
    },
    Template {
        name: "Blank DualShock 4",
        description: "Nothing assigned, on the DualShock 4 photo.",
        toml: include_str!("../../../profiles/templates/blank-dualshock4.toml"),
    },
    Template {
        name: "Xbox controller (all games)",
        description: "Games see an Xbox controller, as with Steam Input. Nothing is assigned yet. \
                      Needs the free ViGEmBus driver.",
        toml: include_str!("../../../profiles/templates/xbox-controller.toml"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_layouts_parse_and_name_their_pictures() {
        for picture in &PICTURES {
            let layout = zpadder_config::parse_layout(picture.layout).unwrap();
            assert_eq!(layout.image.as_deref(), Some(format!("{}.png", picture.id).as_str()));
            assert!(layout.positions.len() >= 16, "{}", picture.id);
            assert!(picture.png.starts_with(b"\x89PNG"), "{}", picture.id);
        }
    }

    #[test]
    fn templates_are_valid_profiles() {
        for template in &TEMPLATES {
            let profile = zpadder_config::parse(template.toml)
                .unwrap_or_else(|e| panic!("{}: {e}", template.name));
            assert_eq!(profile.name, template.name);
            if let Some(layout) = &profile.layout {
                assert!(picture(layout).is_some(), "{}: unknown picture {layout}", template.name);
            }
        }
    }
}
