use serde::{Deserialize, Serialize};

use super::Rgb;

const OUTLINE: Rgb = [42, 34, 48];
const WHITE: Rgb = [255, 246, 233];
const PINK: Rgb = [244, 166, 183];
const RED: Rgb = [224, 67, 90];
// Dédé's own: belly, nostrils, hooves, cheek.
const BELLY: Rgb = [252, 202, 208];
const NOSTRIL: Rgb = [120, 50, 70];
const HOOF: Rgb = [118, 82, 92];
const CHEEK: Rgb = [240, 140, 150];

/// The cat's fur and eyes. The sprites name body parts, each coat colors them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coat {
    /// The original orange tabby.
    #[default]
    Ginger,
    /// Pale lynx point: cream fur, tabby forehead, smoky tail, blue eyes.
    Willow,
    /// Grey and white bicolour: white blaze, muzzle and paws, odd eyes.
    Alfie,
    /// Not a cat: a pink pig with mud on his flank, who poops and eats it.
    Dede,
    /// Not a cat either: a red and white corgi, long, low and big-eared.
    Corgi,
    /// Colors read from a photo of the user's own cat.
    Custom(Palette),
}

/// Colors of the parts that change between coats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub fur: Rgb,
    /// Tabby stripes on the head and back; the fur color when there are none.
    pub stripe: Rgb,
    /// The forehead between the ears.
    pub blaze: Rgb,
    pub muzzle: Rgb,
    pub legs: Rgb,
    pub tail: Rgb,
    pub near_eye: Rgb,
    pub far_eye: Rgb,
}

const GINGER: Palette = Palette {
    fur: [242, 166, 90],
    stripe: [214, 128, 52],
    blaze: [242, 166, 90],
    muzzle: [242, 166, 90],
    legs: [242, 166, 90],
    tail: [242, 166, 90],
    near_eye: OUTLINE,
    far_eye: OUTLINE,
};

const WILLOW: Palette = Palette {
    fur: [222, 210, 198],
    stripe: [148, 134, 128],
    blaze: [128, 114, 110],
    muzzle: WHITE,
    legs: [190, 178, 170],
    tail: [104, 94, 98],
    near_eye: [92, 150, 210],
    far_eye: [92, 150, 210],
};

const ALFIE: Palette = Palette {
    fur: [112, 116, 128],
    stripe: [112, 116, 128],
    blaze: WHITE,
    muzzle: WHITE,
    legs: WHITE,
    tail: [88, 90, 102],
    near_eye: [110, 168, 222],
    far_eye: [214, 158, 56],
};

const DEDE: Palette = Palette {
    fur: [246, 178, 188],
    // Mud.
    stripe: [128, 92, 62],
    blaze: [246, 178, 188],
    // The snout and the inside of the ears.
    muzzle: [226, 124, 146],
    legs: [246, 178, 188],
    tail: [218, 112, 138],
    near_eye: OUTLINE,
    far_eye: OUTLINE,
};

const CORGI: Palette = Palette {
    fur: [222, 140, 62],
    // The darker saddle on the back.
    stripe: [190, 110, 46],
    blaze: WHITE,
    muzzle: WHITE,
    legs: WHITE,
    tail: [222, 140, 62],
    near_eye: [70, 44, 30],
    far_eye: [70, 44, 30],
};

/// The animal a coat is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animal {
    Cat,
    Pig,
    Corgi,
}

impl Coat {
    pub const ALL: [Coat; 5] = [
        Coat::Ginger,
        Coat::Willow,
        Coat::Alfie,
        Coat::Dede,
        Coat::Corgi,
    ];

    /// The name of a built-in coat; custom ones are named in the settings.
    pub fn name(self) -> &'static str {
        match self {
            Coat::Ginger => "Ginger",
            Coat::Willow => "Willow",
            Coat::Alfie => "Alfie",
            Coat::Dede => "Dédé",
            Coat::Corgi => "Corgi",
            Coat::Custom(_) => "My Cat",
        }
    }

    fn palette(&self) -> &Palette {
        match self {
            Coat::Ginger => &GINGER,
            Coat::Willow => &WILLOW,
            Coat::Alfie => &ALFIE,
            Coat::Dede => &DEDE,
            Coat::Corgi => &CORGI,
            Coat::Custom(palette) => palette,
        }
    }

    /// Dédé is drawn as a pig, and poops where a cat coughs up hairballs.
    pub fn is_pig(self) -> bool {
        self == Coat::Dede
    }

    pub fn animal(self) -> Animal {
        match self {
            Coat::Dede => Animal::Pig,
            Coat::Corgi => Animal::Corgi,
            _ => Animal::Cat,
        }
    }

    /// The color of a sprite cell; `None` for anything else (transparent).
    pub(super) fn color(self, c: char) -> Option<Rgb> {
        let palette = self.palette();
        match c {
            'k' => Some(OUTLINE),
            'w' => Some(WHITE),
            'p' => Some(PINK),
            'r' => Some(RED),
            'b' => Some(BELLY),
            'n' => Some(NOSTRIL),
            'h' => Some(HOOF),
            'q' => Some(CHEEK),
            'o' => Some(palette.fur),
            's' => Some(palette.stripe),
            'm' => Some(palette.blaze),
            'c' => Some(palette.muzzle),
            'l' => Some(palette.legs),
            't' => Some(palette.tail),
            'e' => Some(palette.near_eye),
            'f' => Some(palette.far_eye),
            _ => None,
        }
    }
}
