//! What one tick of a live loop allocates once it runs: the motion engine
//! (`Animator`) and the frame it builds, and the copy a pacer keeps.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use glam::Vec2;
use meow::creature::CreatureAsset;
use meow::motion::mixer::Mixer;
use meow::motion::{Animator, Placement, Play};

struct Counting;

thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

// SAFETY: forwards to the system allocator, only counting on the threads
// that asked to.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        // SAFETY: the caller's layout, as given.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated above with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        // SAFETY: as allocated above.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn counted(run: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(Some(0)));
    run();
    COUNT.with(|c| c.replace(None)).unwrap_or(0)
}

const FIXTURE: &[u8] = include_bytes!("fixtures/mini_creature.glb");

/// How an animator plays on: what it is asked each tick.
struct Case {
    name: &'static str,
    start: fn(&mut Animator, &CreatureAsset),
    tick: fn(&mut Animator, u64),
}

fn idle(animator: &mut Animator, _: &CreatureAsset) {
    animator.play("idle", Play::Loop, 1.0, 0.0);
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "idle",
            start: idle,
            tick: |_, _| {},
        },
        Case {
            name: "crossfades",
            start: idle,
            tick: |animator, tick| match tick % 40 {
                0 => {
                    animator.play("nod", Play::Once, 1.0, 0.2);
                }
                1 => {
                    animator.play("idle", Play::Loop, 1.0, 0.2);
                }
                _ => {}
            },
        },
        Case {
            name: "overlay",
            start: |animator, asset| {
                idle(animator, asset);
                let head = asset.skeleton.find("head").unwrap();
                let mask = Mixer::mask_below(&asset.skeleton, head);
                animator.overlay("nod", Play::Loop, 0.2, Some(mask));
            },
            tick: |_, _| {},
        },
    ]
}

#[test]
fn a_steady_tick_allocates_nothing() {
    let asset = CreatureAsset::from_glb(FIXTURE).unwrap();
    let mut placement = Placement::new(Vec2::new(90.0, 20.0), 150.0);
    for case in cases() {
        let mut animator = Animator::new(&asset);
        (case.start)(&mut animator, &asset);
        let mut frame = animator.update(1.0 / 30.0, &placement);
        let mut kept = frame.clone();
        // Settle into it: the clips start, the buffers grow once.
        let mut tick = 0;
        for _ in 0..120 {
            tick += 1;
            (case.tick)(&mut animator, tick);
            animator.update_into(1.0 / 30.0, &placement, &mut frame);
            kept.copy_from(&frame);
        }
        let mut worst = 0;
        for _ in 0..300 {
            tick += 1;
            placement.origin.x += 1.0;
            let n = counted(|| {
                (case.tick)(&mut animator, tick);
                animator.update_into(1.0 / 30.0, &placement, &mut frame);
                kept.copy_from(&frame);
            });
            worst = worst.max(n);
        }
        assert_eq!(worst, 0, "{}: {worst} allocations in a tick", case.name);
        assert_eq!(kept, frame);
    }
}
