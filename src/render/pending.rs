//! The renderer compiled off the main thread: building the shader library
//! and the pipelines takes about 10 ms, a skipped frame or two on the main
//! thread. Metal objects are thread safe, so the renderer made on the
//! worker draws on the main thread.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCopyAllDevices, MTLCreateSystemDefaultDevice, MTLDevice};

use super::Renderer;
use crate::creature::asset::CreatureAsset;

/// A renderer made on the worker thread, handed whole to the main thread.
struct Compiled(Option<Box<Renderer>>);

// SAFETY: Metal's devices, libraries, pipelines, buffers and textures are
// thread safe (objc2-metal only leaves buffers and textures without `Send`
// because of their mutable contents). The worker never touches the renderer
// again once sent: it moves to the main thread with no other reference.
unsafe impl Send for Compiled {}

/// What [`Pending::poll`] hands a window.
pub enum Share {
    /// Not compiled yet: draw nothing for now, ask again next tick.
    Compiling,
    Ready(Box<Renderer>),
    /// Never: no Metal device, or the shaders did not compile (said once
    /// on stderr).
    Unavailable,
}

/// The first renderer, compiled once off the main thread; each window gets
/// a share of it.
pub struct Pending(State);

enum State {
    Compiling(Receiver<Compiled>),
    Done(Option<Box<Renderer>>),
}

impl Pending {
    /// Starts compiling the renderer for `asset` on a worker thread.
    pub fn spawn(asset: CreatureAsset) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("model-shaders".into())
            .spawn(move || {
                let renderer = autoreleasepool(|_| match low_power_device() {
                    None => {
                        eprintln!("meow: no Metal device, no 3D model");
                        None
                    }
                    Some(device) => Renderer::new(&device, &asset)
                        .map(Box::new)
                        .map_err(|error| eprintln!("meow: renderer: {error}"))
                        .ok(),
                });
                let _ = sender.send(Compiled(renderer));
            });
        if let Err(error) = spawned {
            // The receiver sees the sender dropped: unavailable.
            eprintln!("meow: renderer: no thread to compile on: {error}");
        }
        Pending(State::Compiling(receiver))
    }

    /// A renderer of its own for one window, sharing the compiled shaders.
    /// Never waits.
    pub fn poll(&mut self) -> Share {
        let state = &mut self.0;
        if let State::Compiling(receiver) = state {
            match receiver.try_recv() {
                Ok(Compiled(renderer)) => *state = State::Done(renderer),
                Err(TryRecvError::Empty) => return Share::Compiling,
                Err(TryRecvError::Disconnected) => *state = State::Done(None),
            }
        }
        match state {
            State::Done(Some(renderer)) => Share::Ready(Box::new(renderer.share())),
            _ => Share::Unavailable,
        }
    }
}

/// The integrated GPU when the Mac has two (an Intel MacBook Pro): a little
/// creature is no reason to keep the discrete one awake. Else the default.
pub fn low_power_device() -> Option<Retained<ProtocolObject<dyn MTLDevice>>> {
    MTLCopyAllDevices()
        .iter()
        .find(|device| device.isLowPower() && !device.isRemovable())
        .or_else(|| MTLCreateSystemDefaultDevice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/mini_creature.glb");

    #[test]
    fn the_renderer_compiles_off_the_main_thread_and_never_waits() {
        // Not sent yet: asked, it says so at once.
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut pending = Pending(State::Compiling(receiver));
        assert!(matches!(pending.poll(), Share::Compiling));
        // The worker gave up: unavailable for good.
        drop(sender);
        assert!(matches!(pending.poll(), Share::Unavailable));
        assert!(matches!(pending.poll(), Share::Unavailable));

        // The real compile, on its thread.
        let mut pending = Pending::spawn(CreatureAsset::from_glb(FIXTURE).unwrap());
        let started = Instant::now();
        let ready = loop {
            match pending.poll() {
                Share::Compiling => {
                    assert!(started.elapsed().as_secs() < 30, "never compiled");
                    std::thread::yield_now();
                }
                Share::Ready(_) => break true,
                // No Metal device here (a CI runner): nothing to check.
                Share::Unavailable => break false,
            }
        };
        if ready {
            // Each window after gets its own share of it.
            assert!(matches!(pending.poll(), Share::Ready(_)));
        }
    }
}
