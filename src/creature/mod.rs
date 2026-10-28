//! A 3D creature: what its GLB holds ([`asset`]), its poses ([`pose`]) and
//! what the animator hands the renderer each frame ([`frame`]).
//!
//! Pure logic, no AppKit and no Metal, tested with `cargo test`. The motion
//! engine (`crate::motion`) plays its clips, the renderer (`crate::render`)
//! draws it.
//!
//! Space: everything 3D stays in glTF space, in meters: Y up, the creature
//! facing +Z (toward the viewer), its left side (bones `_L`) toward +X, its
//! origin on the ground between the feet. Nothing is converted at load.

pub mod asset;
pub mod frame;
pub mod pose;

pub use asset::CreatureAsset;
pub use frame::Frame;
pub use pose::{Pose, Transform};
