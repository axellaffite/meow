use super::stretch::{SCRATCH_SECONDS, STRETCH_SECONDS};
use super::*;

const IDLE: Inputs = Inputs {
    busy: false,
    chat_open: false,
    unread: false,
    news: false,
};

const SCREEN: Bounds = Bounds {
    min_x: 0.0,
    max_x: 1000.0,
    max_y: 800.0,
    open: OpenEdges::NONE,
};

const TICK: f64 = 1.0 / 60.0;

fn run(cat: &mut Cat, seconds: f64, rng: &mut Rng) {
    for _ in 0..(seconds / TICK).ceil() as usize {
        cat.update(TICK, SCREEN, IDLE, rng);
    }
}

fn idle_cat() -> Cat {
    let mut cat = Cat::new(400.0);
    cat.activity = Activity::Idle;
    cat.time_left = 1000.0;
    cat
}

#[test]
fn a_stretch_eases_in_holds_and_ends_standing() {
    let mut rng = Rng::new(7);
    let mut cat = idle_cat();
    assert!(cat.stretch_out(&mut rng));
    assert_eq!(cat.activity(), Activity::Stretching);
    let x = cat.x;
    let mut poses = Vec::new();
    for _ in 0..(STRETCH_SECONDS / TICK) as usize - 1 {
        run(&mut cat, TICK, &mut rng);
        poses.push(cat.look(IDLE).pose);
    }
    let held = match cat.stretch {
        Stretch::Bow => [Pose::Bow(0), Pose::Bow(1)],
        Stretch::Arch => [Pose::Arch(0), Pose::Arch(1)],
    };
    assert_eq!(poses.first(), Some(&held[0]), "eases in");
    assert!(poses.contains(&held[1]), "all the way");
    assert_eq!(poses.last(), Some(&held[0]), "eases out");
    run(&mut cat, 2.0 * TICK, &mut rng);
    assert_eq!(cat.activity(), Activity::Idle);
    assert_eq!(cat.x, x, "stretching on the spot");
}

#[test]
fn cats_stretch_both_ways() {
    let mut rng = Rng::new(3);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..20 {
        let mut cat = idle_cat();
        assert!(cat.stretch_out(&mut rng));
        seen.insert(cat.stretch);
    }
    assert_eq!(seen.len(), 2);
}

#[test]
fn dede_and_busy_cats_do_not_stretch() {
    let mut rng = Rng::new(1);
    let mut pig = idle_cat();
    pig.set_pig(true);
    assert!(!pig.stretch_out(&mut rng));
    let mut held = idle_cat();
    held.grab();
    assert!(!held.stretch_out(&mut rng));
}

#[test]
fn a_waking_cat_often_stretches() {
    let mut rng = Rng::new(11);
    let stretched = (0..100)
        .filter(|_| {
            let mut cat = idle_cat();
            cat.activity = Activity::Sleeping;
            cat.time_left = 0.0;
            run(&mut cat, TICK, &mut rng);
            cat.activity() == Activity::Stretching
        })
        .count();
    let expected = WAKE_STRETCH_CHANCE * 100.0;
    assert!((stretched as f64 - expected).abs() < 20.0, "{stretched}");
}

#[test]
fn petting_or_a_request_interrupts_a_stretch() {
    let mut rng = Rng::new(5);
    let mut cat = idle_cat();
    assert!(cat.stretch_out(&mut rng));
    let busy = Inputs { busy: true, ..IDLE };
    cat.update(TICK, SCREEN, busy, &mut rng);
    assert_eq!(cat.activity(), Activity::Idle);
}

/// A cat about to walk into the right edge of the screen.
fn cat_at_the_wall() -> Cat {
    let mut cat = Cat::new(SCREEN.max_x - 0.1);
    cat.facing = Facing::Right;
    cat.time_left = 1000.0;
    cat
}

#[test]
fn walking_into_the_wall_the_cat_sometimes_claws_it_then_walks_off() {
    let mut rng = Rng::new(9);
    let mut clawed = 0;
    for _ in 0..100 {
        let mut cat = cat_at_the_wall();
        cat.update(TICK, SCREEN, IDLE, &mut rng);
        if cat.activity() != Activity::Scratching {
            assert_eq!(cat.facing, Facing::Left, "turned back");
            continue;
        }
        clawed += 1;
        assert_eq!(cat.facing, Facing::Right, "facing the wall");
        let look = cat.look(IDLE);
        assert!(matches!(look.pose, Pose::Scratch(_)));
        assert_eq!(look.bubble, Bubble::None);
        run(&mut cat, SCRATCH_SECONDS + TICK, &mut rng);
        assert_eq!(cat.activity(), Activity::Walking);
        assert_eq!(cat.facing, Facing::Left, "walks off the other way");
    }
    let expected = SCRATCH_CHANCE * 100.0;
    assert!((clawed as f64 - expected).abs() < 20.0, "{clawed}");
}

#[test]
fn the_paws_move_while_clawing() {
    let mut rng = Rng::new(2);
    let mut cat = cat_at_the_wall();
    while cat.activity() != Activity::Scratching {
        cat = cat_at_the_wall();
        cat.update(TICK, SCREEN, IDLE, &mut rng);
    }
    let mut poses = Vec::new();
    for _ in 0..30 {
        run(&mut cat, TICK, &mut rng);
        let pose = cat.look(IDLE).pose;
        if !poses.contains(&pose) {
            poses.push(pose);
        }
    }
    assert!(poses.len() >= 3, "{poses:?}");
}

#[test]
fn no_clawing_an_edge_that_leads_on_to_another_screen() {
    let open = Bounds {
        open: OpenEdges {
            right: true,
            ..OpenEdges::NONE
        },
        ..SCREEN
    };
    let mut rng = Rng::new(9);
    for _ in 0..50 {
        let mut cat = cat_at_the_wall();
        cat.update(TICK, open, IDLE, &mut rng);
        assert_ne!(cat.activity(), Activity::Scratching);
    }
}

#[test]
fn dede_does_not_claw_walls() {
    let mut rng = Rng::new(9);
    for _ in 0..50 {
        let mut pig = cat_at_the_wall();
        pig.set_pig(true);
        pig.update(TICK, SCREEN, IDLE, &mut rng);
        assert_ne!(pig.activity(), Activity::Scratching);
    }
}
