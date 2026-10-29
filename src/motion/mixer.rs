//! Clip playback: the clips by name, tracks that play them (looped, once,
//! held on their last frame, backwards), and two layers that crossfade
//! tracks, the base and an overlay drawn over it through a bone mask.
//!
//! Sampling is the asset's (`Clip::sample`: linear, step and cubic spline as
//! exported). Discrete data bones (a `ctrl_*` bone whose translation says
//! which expression or mode is on) are never averaged: halfway between mode
//! 0 and mode 4 is not mode 2, so they come from the track that weighs most.

use glam::Vec3;

use crate::creature::asset::{
    CTRL_PREFIX, ChannelPath, Clip, CreatureAsset, Interpolation, Skeleton,
};
use crate::creature::pose::Pose;

use super::rig::descends;

/// A clip of the [`Library`], by its index in the asset's clips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClipId(pub usize);

/// The asset's clips, and how their data bones play.
#[derive(Clone, Debug)]
pub struct Library {
    clips: Vec<Clip>,
    /// The discrete data bones (see [`Library::set_discrete`]).
    discrete: Vec<usize>,
    /// Per clip, the translation channels of discrete bones that were not
    /// exported as steps: they hold each key until the next, as if they were.
    held: Vec<Vec<usize>>,
}

impl Library {
    /// The asset's clips. A `ctrl_*` bone whose translation is a step
    /// channel in any clip is discrete.
    pub fn new(asset: &CreatureAsset) -> Self {
        let mut discrete: Vec<usize> = asset
            .clips
            .iter()
            .flat_map(|clip| &clip.channels)
            .filter(|c| {
                c.path() == ChannelPath::Translation && c.interpolation == Interpolation::Step
            })
            .map(|c| c.joint)
            .filter(|&joint| {
                asset
                    .skeleton
                    .joints
                    .get(joint)
                    .is_some_and(|j| j.name.starts_with(CTRL_PREFIX))
            })
            .collect();
        discrete.sort_unstable();
        discrete.dedup();
        let mut library = Self {
            clips: asset.clips.clone(),
            discrete,
            held: Vec::new(),
        };
        library.find_held();
        library
    }

    /// Makes the translation of `joint` discrete: never averaged, and held
    /// from key to key even where the export baked its steps to linear keys
    /// (Blender's forced sampling does).
    pub fn set_discrete(&mut self, joint: usize) {
        if !self.discrete.contains(&joint) {
            self.discrete.push(joint);
            self.find_held();
        }
    }

    /// The discrete data bones.
    pub fn discrete(&self) -> &[usize] {
        &self.discrete
    }

    fn find_held(&mut self) {
        self.held = self
            .clips
            .iter()
            .map(|clip| {
                clip.channels
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| {
                        c.path() == ChannelPath::Translation
                            && c.interpolation != Interpolation::Step
                            && self.discrete.contains(&c.joint)
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .collect();
    }

    /// The clip named `name`.
    pub fn find(&self, name: &str) -> Option<ClipId> {
        self.clips.iter().position(|c| c.name == name).map(ClipId)
    }

    pub fn name(&self, id: ClipId) -> Option<&str> {
        self.clips.get(id.0).map(|c| c.name.as_str())
    }

    /// Seconds; 0 for a clip it does not have.
    pub fn duration(&self, id: ClipId) -> f32 {
        match self.clips.get(id.0) {
            Some(clip) if clip.duration.is_finite() => clip.duration.max(0.0),
            _ => 0.0,
        }
    }

    /// Writes the clip at `time` into `pose`, over `rest` (joints the clip
    /// leaves alone stay at rest; a clip it does not have gives the rest).
    pub fn sample(&self, id: ClipId, time: f32, rest: &Pose, pose: &mut Pose) {
        pose.locals.clone_from(&rest.locals);
        let Some(clip) = self.clips.get(id.0) else {
            return;
        };
        clip.sample(time, pose);
        for &index in &self.held[id.0] {
            let channel = &clip.channels[index];
            let Some(local) = pose.locals.get_mut(channel.joint) else {
                continue;
            };
            // The key at or before `time`.
            let at = channel.times.partition_point(|&t| t <= time + 1e-4).max(1) - 1;
            if let Some(&key) = channel.times.get(at) {
                channel.apply(key, local);
            }
        }
    }
}

/// How a track runs through its clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Play {
    Loop,
    /// Once, then it holds the last frame and says it is done.
    Once,
    /// Once backwards, from the end to the start.
    Reverse,
}

/// A clip playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub clip: ClipId,
    pub play: Play,
    /// Seconds into the clip.
    pub time: f32,
    pub speed: f32,
    /// 0 .. 1 before easing.
    pub weight: f32,
    /// Weight per second, toward `target`.
    rate: f32,
    target: f32,
    /// Seconds since it started.
    pub age: f32,
    /// Times it went round (loops).
    pub laps: u32,
    /// Started this frame: it shows its first pose as the frame ends, so a
    /// change made at a frame's end starts there, at any frame rate.
    fresh: bool,
    /// Seconds before the weight starts to move (and, for a track fading
    /// in, before its clip starts).
    delay: f32,
    /// Told to fade this frame: like a fresh track's start, the fade
    /// starts as the frame ends, at any frame rate.
    fading: bool,
    /// A fade out asked for later, while it still comes in: it goes on
    /// coming in (a one-shot asked for just before the clip that follows it
    /// still shows) and fades out on time.
    out: Option<Out>,
}

/// A fade out to come.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Out {
    /// Seconds before it starts.
    left: f32,
    /// Weight per second, then.
    rate: f32,
    /// Asked for this frame: it counts from the frame's end.
    armed: bool,
}

impl Track {
    pub fn new(clip: ClipId, play: Play, speed: f32, start: f32, fade: f32) -> Self {
        let (weight, rate) = if fade > 1e-4 {
            (0.0, 1.0 / fade)
        } else {
            (1.0, 0.0)
        };
        Self {
            clip,
            play,
            time: start,
            speed,
            weight,
            rate,
            target: 1.0,
            age: 0.0,
            laps: 0,
            fresh: true,
            delay: 0.0,
            fading: false,
            out: None,
        }
    }

    /// Played to its end (never for a loop).
    pub fn done(&self, library: &Library) -> bool {
        match self.play {
            Play::Loop => false,
            Play::Once => self.time >= library.duration(self.clip) - 1e-4,
            Play::Reverse => self.time <= 1e-4,
        }
    }

    /// Seconds until it is done, at its speed, the wait before it fades in
    /// included: a loop is done once it has faded in.
    pub fn remaining(&self, library: &Library) -> f32 {
        let speed = self.speed.abs().max(1e-3);
        let left = match self.play {
            Play::Loop => 0.0,
            Play::Once => library.duration(self.clip) - self.time,
            Play::Reverse => self.time,
        };
        let wait = if self.target > 0.0 { self.delay } else { 0.0 };
        left.max(0.0) / speed + wait.max(0.0)
    }

    /// Whether it is fading in or playing, not on its way out.
    pub fn is_incoming(&self) -> bool {
        self.target > 0.0
    }

    fn advance(&mut self, dt: f32, library: &Library) {
        if self.fresh {
            // Told to fade before it even started: that fade, too, starts
            // as this frame ends.
            self.fresh = false;
            self.fading = false;
            if let Some(out) = &mut self.out {
                out.armed = true;
            }
            return;
        }
        let waited = dt.min(self.delay.max(0.0));
        // A track waiting to fade in waits to start its clip too; one on
        // its way out plays on meanwhile.
        let playing = if self.target > 0.0 { dt - waited } else { dt };
        self.age += dt;
        let duration = library.duration(self.clip);
        let step = playing * self.speed.abs();
        match self.play {
            Play::Loop => {
                let raw = self.time + step;
                if duration > 0.0 && raw >= duration {
                    self.laps += (raw / duration) as u32;
                    self.time = raw.rem_euclid(duration);
                } else {
                    self.time = raw;
                }
            }
            Play::Once => self.time = (self.time + step).min(duration),
            Play::Reverse => self.time = (self.time - step).max(0.0),
        }
        if !self.time.is_finite() {
            self.time = 0.0;
        }
        if std::mem::take(&mut self.fading) {
            return;
        }
        self.delay -= waited;
        match self.out {
            Some(Out { armed: false, .. }) => {
                self.out = self.out.map(|out| Out { armed: true, ..out });
                self.move_weight(dt - waited);
            }
            Some(out) if out.left > dt => {
                self.out = Some(Out {
                    left: out.left - dt,
                    ..out
                });
                self.move_weight(dt - waited);
            }
            Some(out) => {
                // In until `left`, then out for the rest of the frame.
                let left = out.left.max(waited);
                self.move_weight(left - waited);
                self.out = None;
                self.target = 0.0;
                self.rate = out.rate;
                self.move_weight(dt - left);
            }
            None => self.move_weight(dt - waited),
        }
    }

    /// Moves the weight `dt` seconds toward its target.
    fn move_weight(&mut self, dt: f32) {
        let w = self.weight + (self.target - self.weight).signum() * self.rate * dt.max(0.0);
        self.weight = if self.rate <= 0.0 {
            self.target
        } else if self.target >= self.weight {
            w.min(self.target)
        } else {
            w.max(self.target)
        };
    }

    fn fade_out(&mut self, fade: f32, delay: f32) {
        let rate = if fade > 1e-4 { 1.0 / fade } else { 1e6 };
        if self.target <= 0.0 {
            // On its way out already: only a fade asked for now changes it.
            if delay <= 0.0 {
                self.rate = rate;
                self.delay = 0.0;
            }
            return;
        }
        if delay > 0.0 {
            // Still in: it stays so until then (or until a sooner fade).
            let left = self.out.map_or(delay, |out| out.left.min(delay));
            self.out = Some(Out {
                left,
                rate,
                armed: self.out.is_some_and(|out| out.armed && out.left <= delay),
            });
            return;
        }
        // Only the call that starts the fade waits for the frame's end: one
        // made each frame must not hold it forever.
        self.fading = true;
        self.target = 0.0;
        self.rate = rate;
        self.delay = 0.0;
        self.out = None;
    }

    /// The eased weight.
    pub fn eased(&self) -> f32 {
        let w = self.weight.clamp(0.0, 1.0);
        w * w * (3.0 - 2.0 * w)
    }
}

/// Tracks crossfading: the newest fades in, the others out.
#[derive(Clone, Debug, Default)]
pub struct Layer {
    pub tracks: Vec<Track>,
}

/// The most tracks a layer keeps: past it the oldest goes.
pub const MAX_TRACKS: usize = 4;

impl Layer {
    /// Starts `track`, fading the others out over its fade.
    pub fn push(&mut self, track: Track, fade: f32) {
        self.push_after(track, fade, 0.0);
    }

    /// Starts `track`, the crossfade beginning `delay` seconds from now
    /// (the others play on until then).
    pub fn push_after(&mut self, mut track: Track, fade: f32, delay: f32) {
        let delay = if delay.is_finite() {
            delay.max(0.0)
        } else {
            0.0
        };
        for old in &mut self.tracks {
            old.fade_out(fade, delay);
        }
        track.delay = delay;
        self.tracks.push(track);
        if self.tracks.len() > MAX_TRACKS {
            self.tracks.remove(0);
        }
    }

    pub fn current(&self) -> Option<&Track> {
        self.tracks.last()
    }

    pub fn current_mut(&mut self) -> Option<&mut Track> {
        self.tracks.last_mut()
    }

    /// Fades everything out.
    pub fn stop(&mut self, fade: f32) {
        for track in &mut self.tracks {
            track.fade_out(fade, 0.0);
        }
    }

    pub fn is_silent(&self) -> bool {
        self.tracks
            .iter()
            .all(|t| t.target <= 0.0 && t.weight <= 0.0)
    }

    /// Advances every track, and lets go of those faded out.
    pub fn advance(&mut self, dt: f32, library: &Library) {
        for track in &mut self.tracks {
            track.advance(dt, library);
        }
        self.tracks.retain(|t| t.target > 0.0 || t.weight > 0.0);
    }

    /// Blends the layer into `out`; false when it has no weight. `scratch`
    /// holds one sampled pose.
    fn blend(
        &self,
        library: &Library,
        rest: &Pose,
        scratch: &mut Pose,
        out: &mut Pose,
        discrete: &mut Discrete,
    ) -> bool {
        let mut total = 0.0;
        let mut heaviest = 0.0;
        for track in &self.tracks {
            let w = track.eased();
            if w <= 0.0 {
                continue;
            }
            library.sample(track.clip, track.time, rest, scratch);
            total += w;
            if total <= w {
                out.locals.clone_from(&scratch.locals);
            } else {
                let f = w / total;
                for (o, s) in out.locals.iter_mut().zip(&scratch.locals) {
                    *o = o.blend(s, f);
                }
            }
            if w > heaviest {
                heaviest = w;
                discrete.read(library.discrete(), scratch);
            }
        }
        total > 0.0
    }

    /// The summed eased weight of tracks playing `clip`.
    pub fn weight_of(&self, clip: ClipId) -> f32 {
        self.tracks
            .iter()
            .filter(|t| t.clip == clip)
            .map(Track::eased)
            .sum::<f32>()
            .min(1.0)
    }
}

/// The discrete data bones' translations, kept whole, in the order of
/// [`Library::discrete`].
#[derive(Clone, Debug, Default)]
struct Discrete {
    values: Vec<Vec3>,
}

impl Discrete {
    fn read(&mut self, joints: &[usize], pose: &Pose) {
        self.values.clear();
        self.values.extend(
            joints
                .iter()
                .map(|&j| pose.locals.get(j).map_or(Vec3::ZERO, |t| t.t)),
        );
    }

    fn write(&self, joints: &[usize], pose: &mut Pose) {
        for (&j, &value) in joints.iter().zip(&self.values) {
            if let Some(t) = pose.locals.get_mut(j) {
                t.t = value;
            }
        }
    }
}

/// The base layer and an overlay over it.
#[derive(Clone, Debug)]
pub struct Mixer {
    pub base: Layer,
    pub overlay: Layer,
    /// The overlay's weight per joint (see [`Mixer::mask_below`]); `None`
    /// covers every joint.
    pub mask: Option<Vec<f32>>,
    scratch: Pose,
    over: Pose,
    base_discrete: Discrete,
    over_discrete: Discrete,
}

impl Mixer {
    pub fn new(rest: &Pose) -> Self {
        Self {
            base: Layer::default(),
            overlay: Layer::default(),
            mask: None,
            scratch: rest.clone(),
            over: rest.clone(),
            base_discrete: Discrete::default(),
            over_discrete: Discrete::default(),
        }
    }

    /// A mask covering `bone` and every bone under it: an overlay on the
    /// head leaves the legs to the base.
    pub fn mask_below(skeleton: &Skeleton, bone: usize) -> Vec<f32> {
        (0..skeleton.joints.len())
            .map(|i| f32::from(u8::from(descends(skeleton, i, bone))))
            .collect()
    }

    /// The blended pose, into `out`.
    pub fn evaluate(&mut self, library: &Library, rest: &Pose, out: &mut Pose) {
        let joints = library.discrete();
        self.base_discrete.values.clear();
        if !self.base.blend(
            library,
            rest,
            &mut self.scratch,
            out,
            &mut self.base_discrete,
        ) {
            out.locals.clone_from(&rest.locals);
        }
        self.base_discrete.write(joints, out);
        let weight = self
            .overlay
            .tracks
            .iter()
            .map(Track::eased)
            .fold(0.0, f32::max);
        if weight <= 0.0 {
            return;
        }
        self.over_discrete.values.clear();
        if !self.overlay.blend(
            library,
            rest,
            &mut self.scratch,
            &mut self.over,
            &mut self.over_discrete,
        ) {
            return;
        }
        self.over_discrete.write(joints, &mut self.over);
        let mask = self.mask.as_deref();
        let masked = |i: usize| mask.map_or(1.0, |m| m.get(i).copied().unwrap_or(0.0));
        for (i, (o, s)) in out.locals.iter_mut().zip(&self.over.locals).enumerate() {
            let w = weight * masked(i);
            if w > 0.0 {
                *o = o.blend(s, w);
            }
        }
        // Discrete bones switch whole, once the overlay is more than half there.
        for (k, &j) in joints.iter().enumerate() {
            let from = if weight * masked(j) >= 0.5 {
                &self.over_discrete
            } else {
                &self.base_discrete
            };
            if let (Some(t), Some(&value)) = (out.locals.get_mut(j), from.values.get(k)) {
                t.t = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/mini_creature.glb");

    fn fixture() -> (CreatureAsset, Library) {
        let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
        let library = Library::new(&asset);
        (asset, library)
    }

    #[test]
    fn step_data_channels_baked_linear_still_step() {
        let mut asset = CreatureAsset::from_glb(FIXTURE).unwrap();
        let face = asset.skeleton.find("ctrl_face").unwrap();
        // The step keys, baked to linear ones as forced sampling does.
        let nod = asset.clips.iter_mut().find(|c| c.name == "nod").unwrap();
        for channel in &mut nod.channels {
            if channel.joint == face && channel.path() == ChannelPath::Translation {
                channel.interpolation = Interpolation::Linear;
            }
        }
        let mut library = Library::new(&asset);
        assert!(library.discrete().is_empty(), "nothing says it is discrete");
        library.set_discrete(face);
        let rest = Pose::rest(&asset.skeleton);
        let mut pose = rest.clone();
        let id = library.find("nod").unwrap();
        // Between frame 0 and frame 10 (the jump to 3), the value holds.
        library.sample(id, 5.0 / 30.0, &rest, &mut pose);
        let held = pose.locals[face].t - rest.locals[face].t;
        assert!(held.length() < 1e-4, "{held}");
    }

    #[test]
    fn the_library_samples_the_clips_over_rest() {
        let (asset, library) = fixture();
        let rest = Pose::rest(&asset.skeleton);
        let head = asset.skeleton.find("head").unwrap();
        let face = asset.skeleton.find("ctrl_face").unwrap();
        assert_eq!(library.discrete(), [face], "its step keys say so");
        let idle = library.find("idle").unwrap();
        let mut pose = rest.clone();
        // idle lifts the head 6 mm at frame 15 of 30.
        library.sample(idle, 0.5, &rest, &mut pose);
        let lift = pose.locals[head].t.y - rest.locals[head].t.y;
        assert!((lift - 0.006).abs() < 1e-4, "{lift}");
        library.sample(idle, 0.25, &rest, &mut pose);
        let half = pose.locals[head].t.y - rest.locals[head].t.y;
        assert!(half > 0.001 && half < 0.006, "{half}");
        // A clip it does not have samples the rest pose and lasts nothing.
        assert_eq!(library.find("dance"), None);
        library.sample(ClipId(99), 0.3, &rest, &mut pose);
        assert_eq!(pose, rest);
        assert_eq!(library.duration(ClipId(99)), 0.0);
    }

    #[test]
    fn a_crossfade_blends_between_the_clips_then_lets_the_old_go() {
        let (asset, library) = fixture();
        let rest = Pose::rest(&asset.skeleton);
        let head = asset.skeleton.find("head").unwrap();
        let (idle, nod) = (library.find("idle").unwrap(), library.find("nod").unwrap());
        let mut mixer = Mixer::new(&rest);
        mixer
            .base
            .push(Track::new(idle, Play::Loop, 0.0, 0.5, 0.0), 0.0);
        mixer.base.advance(0.0, &library);
        mixer
            .base
            .push(Track::new(nod, Play::Loop, 0.0, 0.0, 0.2), 0.2);
        mixer.base.advance(0.0, &library);
        mixer.base.advance(0.1, &library);
        let mut pose = rest.clone();
        mixer.evaluate(&library, &rest, &mut pose);
        let lift = pose.locals[head].t.y - rest.locals[head].t.y;
        assert!(lift > 0.001 && lift < 0.005, "half way: {lift}");
        mixer.base.advance(0.2, &library);
        assert_eq!(mixer.base.tracks.len(), 1, "the old track is gone");
    }

    #[test]
    fn a_delayed_crossfade_waits_then_fades() {
        let (_, library) = fixture();
        let (idle, nod) = (library.find("idle").unwrap(), library.find("nod").unwrap());
        let mut layer = Layer::default();
        layer.push(Track::new(idle, Play::Loop, 1.0, 0.0, 0.0), 0.0);
        layer.push_after(Track::new(nod, Play::Loop, 1.0, 0.0, 0.2), 0.2, 0.1);
        for _ in 0..3 {
            layer.advance(0.03, &library);
        }
        assert_eq!(layer.weight_of(idle), 1.0, "it waits");
        assert_eq!(layer.tracks[1].time, 0.0, "the new clip waits to start");
        for _ in 0..12 {
            layer.advance(0.03, &library);
        }
        assert_eq!(layer.weight_of(nod), 1.0);
        assert_eq!(layer.tracks.len(), 1);
    }

    #[test]
    fn the_old_clip_and_the_new_cross_from_the_frame_end_at_any_frame_rate() {
        let (_, library) = fixture();
        let (idle, nod) = (library.find("idle").unwrap(), library.find("nod").unwrap());
        let weights = |dt: f32| {
            let mut layer = Layer::default();
            layer.push(Track::new(idle, Play::Loop, 1.0, 0.0, 0.0), 0.0);
            layer.advance(dt, &library);
            layer.push(Track::new(nod, Play::Loop, 1.0, 0.0, 0.3), 0.3);
            // The frame the change comes in, then a sixth of a second.
            layer.advance(dt, &library);
            for _ in 0..(1.0 / 6.0 / dt).round() as usize {
                layer.advance(dt, &library);
            }
            (layer.weight_of(idle), layer.weight_of(nod))
        };
        let (old, new) = weights(1.0 / 120.0);
        for dt in [1.0 / 30.0, 1.0 / 12.0] {
            let (o, n) = weights(dt);
            assert!((o - old).abs() < 1e-3, "dt {dt}: the old at {o}, not {old}");
            assert!((n - new).abs() < 1e-3, "dt {dt}: the new at {n}, not {new}");
        }
        // And a fade asked again each frame still goes on.
        let mut layer = Layer::default();
        layer.push(Track::new(idle, Play::Loop, 1.0, 0.0, 0.0), 0.0);
        for _ in 0..20 {
            layer.stop(0.2);
            layer.advance(1.0 / 30.0, &library);
        }
        assert!(layer.tracks.is_empty(), "it faded out");
    }

    #[test]
    fn an_overlay_moves_only_the_masked_bones_and_switches_data_whole() {
        let (asset, library) = fixture();
        let skeleton = &asset.skeleton;
        let rest = Pose::rest(skeleton);
        let (body, face) = (
            skeleton.find("body").unwrap(),
            skeleton.find("ctrl_face").unwrap(),
        );
        let (idle, nod) = (library.find("idle").unwrap(), library.find("nod").unwrap());
        let mut mixer = Mixer::new(&rest);
        mixer
            .base
            .push(Track::new(idle, Play::Loop, 0.0, 0.0, 0.0), 0.0);
        let mut mask = Mixer::mask_below(skeleton, skeleton.find("head").unwrap());
        mask[face] = 1.0;
        mixer.mask = Some(mask);
        // Nod at frame 10, a third of the way in.
        let mut track = Track::new(nod, Play::Loop, 0.0, 1.0 / 3.0, 0.0);
        track.weight = 0.4;
        track.rate = 0.0;
        track.target = 0.4;
        mixer.overlay.tracks.push(track);
        let mut pose = rest.clone();
        mixer.evaluate(&library, &rest, &mut pose);
        assert_eq!(
            pose.locals[body], rest.locals[body],
            "the body is not under the head"
        );
        assert_eq!(
            pose.locals[face].t, rest.locals[face].t,
            "under half there: the base's data"
        );
        mixer.overlay.tracks[0].weight = 0.8;
        mixer.overlay.tracks[0].target = 0.8;
        mixer.evaluate(&library, &rest, &mut pose);
        let data = pose.locals[face].t - rest.locals[face].t;
        assert!(
            (data - Vec3::new(3.0, 0.5, -0.25)).length() < 1e-4,
            "the overlay's, whole: {data}"
        );
    }
}
