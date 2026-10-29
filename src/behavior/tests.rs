use super::*;

const IDLE: Inputs = Inputs {
    busy: false,
    chat_open: false,
    unread: false,
    news: false,
};

#[test]
fn walking_cat_moves_toward_facing() {
    let mut cat = Cat::new(100.0);
    cat.update(1.0, Bounds::new(0.0, 500.0, 1000.0), IDLE, &mut Rng::new(1));
    assert!((cat.x - (100.0 + WALK_SPEED)).abs() < 1e-9);
}

/// Walks the cat for a second, then lets it finish clawing the wall if it
/// stopped to.
fn walk_into_the_edge(cat: &mut Cat) {
    let bounds = Bounds::new(0.0, 500.0, 1000.0);
    let mut rng = Rng::new(1);
    cat.update(1.0, bounds, IDLE, &mut rng);
    while cat.activity() == Activity::Scratching {
        cat.update(0.1, bounds, IDLE, &mut rng);
    }
}

#[test]
fn cat_turns_around_at_right_edge() {
    let mut cat = Cat::new(495.0);
    walk_into_the_edge(&mut cat);
    assert_eq!(cat.x, 500.0);
    assert_eq!(cat.facing, Facing::Left);
}

#[test]
fn cat_turns_around_at_left_edge() {
    let mut cat = Cat::new(5.0);
    cat.facing = Facing::Left;
    walk_into_the_edge(&mut cat);
    assert_eq!(cat.x, 0.0);
    assert_eq!(cat.facing, Facing::Right);
}

#[test]
fn busy_cat_stays_put_and_shows_dots() {
    let mut cat = Cat::new(100.0);
    let busy = Inputs { busy: true, ..IDLE };
    cat.update(1.0, Bounds::new(0.0, 500.0, 1000.0), busy, &mut Rng::new(1));
    assert_eq!(cat.x, 100.0);
    assert_eq!(cat.activity(), Activity::Idle);
    assert!(matches!(cat.look(busy).bubble, Bubble::Dots(1..=3)));
}

#[test]
fn open_chat_holds_the_cat() {
    let mut cat = Cat::new(100.0);
    cat.update(
        2.0,
        Bounds::new(0.0, 500.0, 1000.0),
        Inputs {
            chat_open: true,
            ..IDLE
        },
        &mut Rng::new(1),
    );
    assert_eq!(cat.x, 100.0);
}

#[test]
fn unread_reply_shows_bang() {
    let cat = Cat::new(100.0);
    let look = cat.look(Inputs {
        unread: true,
        ..IDLE
    });
    assert_eq!(look.bubble, Bubble::Bang);
}

#[test]
fn news_shows_the_bubble_without_keeping_the_cat_put() {
    let news = Inputs { news: true, ..IDLE };
    assert_eq!(Cat::new(100.0).look(news).bubble, Bubble::Bang);
    assert!(!news.holds_cat());
}

#[test]
fn position_is_clamped_when_screen_shrinks() {
    let mut cat = Cat::new(900.0);
    cat.update(
        0.0,
        Bounds::new(0.0, 300.0, 1000.0),
        Inputs { busy: true, ..IDLE },
        &mut Rng::new(1),
    );
    assert_eq!(cat.x, 300.0);
}

#[test]
fn cat_eventually_rests_then_walks_again() {
    let mut cat = Cat::new(100.0);
    let mut rng = Rng::new(42);
    let mut seen = Vec::new();
    for _ in 0..20_000 {
        cat.update(0.05, Bounds::new(0.0, 1000.0, 1000.0), IDLE, &mut rng);
        if seen.last() != Some(&cat.activity()) {
            seen.push(cat.activity());
        }
    }
    assert!(seen.contains(&Activity::Idle));
    assert!(seen.contains(&Activity::Sleeping));
    assert!(seen.iter().filter(|a| **a == Activity::Walking).count() > 1);
}

#[test]
fn walk_animation_cycles_through_phases() {
    let mut cat = Cat::new(100.0);
    let mut phases = std::collections::HashSet::new();
    for _ in 0..8 {
        cat.update(
            1.0 / WALK_FPS,
            Bounds::new(0.0, 1000.0, 1000.0),
            IDLE,
            &mut Rng::new(3),
        );
        if let Pose::Walk(p) = cat.look(IDLE).pose {
            phases.insert(p);
        }
    }
    assert_eq!(phases.len(), 4);
}

#[test]
fn rng_range_stays_in_bounds() {
    let mut rng = Rng::new(7);
    assert!(
        (0..1000)
            .map(|_| rng.range(2.0, 5.0))
            .all(|v| (2.0..5.0).contains(&v))
    );
}

const SCREEN: Bounds = Bounds {
    min_x: 0.0,
    max_x: 1000.0,
    max_y: 800.0,
    open: OpenEdges::NONE,
};

fn run(cat: &mut Cat, seconds: f64, rng: &mut Rng) {
    for _ in 0..(seconds * 60.0) as usize {
        cat.update(1.0 / 60.0, SCREEN, IDLE, rng);
    }
}

fn held_cat_at(x: f64, y: f64) -> Cat {
    let mut cat = Cat::new(x);
    cat.grab();
    cat.drag_to(x, y);
    cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    // Stand still a moment so no throw velocity is left.
    for _ in 0..30 {
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    }
    cat
}

#[test]
fn held_cat_follows_the_drag_and_stays_up() {
    let mut cat = held_cat_at(300.0, 400.0);
    run(&mut cat, 1.0, &mut Rng::new(1));
    assert_eq!((cat.x, cat.y), (300.0, 400.0));
    assert!(matches!(cat.look(IDLE).pose, Pose::Dangle { .. }));
}

#[test]
fn held_cat_is_kept_on_screen() {
    let mut cat = Cat::new(100.0);
    cat.grab();
    cat.drag_to(-50.0, 5000.0);
    cat.update(0.016, SCREEN, IDLE, &mut Rng::new(1));
    assert_eq!((cat.x, cat.y), (0.0, 800.0));
}

#[test]
fn dropped_cat_falls_to_the_ground_and_lands_idle() {
    let mut cat = held_cat_at(300.0, 400.0);
    cat.release();
    let mut rng = Rng::new(1);
    cat.update(0.05, SCREEN, IDLE, &mut rng);
    assert_eq!(cat.activity(), Activity::Falling);
    assert_eq!(cat.look(IDLE).pose, Pose::Fall);
    assert!(cat.y < 400.0);

    run(&mut cat, 1.0, &mut rng);
    assert_eq!(cat.y, 0.0);
    assert_eq!(cat.activity(), Activity::Idle);
    assert!(
        (cat.x - 300.0).abs() < 1.0,
        "a plain drop falls straight down"
    );
}

#[test]
fn fall_time_matches_gravity() {
    // From 400 pt, t = sqrt(2h/g) ≈ 0.55 s.
    let mut cat = held_cat_at(300.0, 400.0);
    cat.release();
    let mut rng = Rng::new(1);
    let mut elapsed = 0.0;
    while cat.activity() == Activity::Falling {
        cat.update(1.0 / 240.0, SCREEN, IDLE, &mut rng);
        elapsed += 1.0 / 240.0;
    }
    let expected = (2.0 * 400.0 / GRAVITY).sqrt();
    assert!((elapsed - expected).abs() < 0.02, "{elapsed} vs {expected}");
}

#[test]
fn thrown_cat_keeps_momentum_and_faces_the_throw() {
    let mut cat = held_cat_at(300.0, 400.0);
    for step in 1..=5 {
        cat.drag_to(300.0 + step as f64 * 10.0, 400.0);
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    }
    cat.release();
    assert_eq!(cat.facing, Facing::Right);
    run(&mut cat, 2.0, &mut Rng::new(1));
    assert!(cat.x > 400.0, "throw carried the cat to x = {}", cat.x);
    assert_eq!(cat.y, 0.0);
}

#[test]
fn gently_thrown_cat_bounces_off_the_screen_edge() {
    let mut cat = held_cat_at(950.0, 700.0);
    for step in 1..=5 {
        cat.drag_to(950.0 + step as f64 * 3.0, 700.0);
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    }
    cat.release();
    run(&mut cat, 2.0, &mut Rng::new(1));
    assert!(cat.x < 1000.0, "bounced back to x = {}", cat.x);
}

#[test]
fn busy_cat_still_falls() {
    let mut cat = held_cat_at(300.0, 400.0);
    cat.release();
    let busy = Inputs { busy: true, ..IDLE };
    for _ in 0..120 {
        cat.update(1.0 / 60.0, SCREEN, busy, &mut Rng::new(1));
    }
    assert_eq!(cat.y, 0.0);
    assert!(matches!(cat.look(busy).bubble, Bubble::Dots(_)));
}

#[test]
fn release_without_grab_does_nothing() {
    let mut cat = Cat::new(100.0);
    cat.release();
    cat.drag_to(500.0, 500.0);
    assert_eq!(cat.activity(), Activity::Walking);
    assert_eq!((cat.x, cat.y), (100.0, 0.0));
}

fn swing_of(cat: &Cat) -> i8 {
    match cat.look(IDLE).pose {
        Pose::Dangle { swing, .. } => swing,
        other => panic!("not dangling: {other:?}"),
    }
}

fn drag_right(cat: &mut Cat, steps: usize, speed: f64) {
    for _ in 0..steps {
        cat.drag_to(cat.x + speed / 60.0, cat.y);
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    }
}

#[test]
fn held_cat_trails_behind_the_drag() {
    let mut cat = held_cat_at(300.0, 400.0);
    drag_right(&mut cat, 60, 600.0);
    assert!(
        swing_of(&cat) < 0,
        "body should trail left, swing = {}",
        swing_of(&cat)
    );
}

#[test]
fn held_cat_overshoots_then_settles_when_drag_stops() {
    let mut cat = held_cat_at(300.0, 400.0);
    drag_right(&mut cat, 60, 600.0);
    let mut swings = Vec::new();
    for _ in 0..180 {
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
        swings.push(swing_of(&cat));
    }
    assert!(
        swings.iter().any(|&s| s > 0),
        "should swing past vertical: {swings:?}"
    );
    assert_eq!(swings.last(), Some(&0));
}

#[test]
fn swing_is_mirrored_for_left_facing_cat() {
    let mut cat = held_cat_at(300.0, 400.0);
    drag_right(&mut cat, 60, 600.0);
    let right = swing_of(&cat);
    cat.facing = Facing::Left;
    assert_eq!(swing_of(&cat), -right);
}

#[test]
fn grabbed_cat_kicks_then_goes_limp() {
    let mut cat = Cat::new(300.0);
    cat.grab();
    let mut kicks = std::collections::HashSet::new();
    for _ in 0..30 {
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
        if let Pose::Dangle { kick, .. } = cat.look(IDLE).pose {
            kicks.insert(format!("{kick:?}"));
        }
    }
    assert!(
        kicks.contains("Left") && kicks.contains("Right"),
        "{kicks:?}"
    );
    run(&mut cat, 1.0, &mut Rng::new(1));
    assert!(matches!(
        cat.look(IDLE).pose,
        Pose::Dangle {
            kick: Kick::None,
            ..
        }
    ));
}

#[test]
fn held_cat_hides_its_bubble() {
    let cat = held_cat_at(300.0, 400.0);
    let busy = Inputs { busy: true, ..IDLE };
    assert_eq!(cat.look(busy).bubble, Bubble::None);
}

/// A cat flying toward the right edge at `speed`, released at `height`.
fn thrown_right(speed: f64, height: f64) -> Cat {
    let mut cat = held_cat_at(700.0, height);
    for _ in 0..10 {
        cat.drag_to(cat.x + speed / 60.0, height);
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    }
    cat.release();
    cat
}

fn run_until(cat: &mut Cat, activity: Activity, seconds: f64) -> bool {
    let mut rng = Rng::new(1);
    for _ in 0..(seconds * 240.0) as usize {
        cat.update(1.0 / 240.0, SCREEN, IDLE, &mut rng);
        if cat.activity() == activity {
            return true;
        }
    }
    false
}

#[test]
fn hard_throw_sticks_the_cat_to_the_wall_facing_it() {
    let mut cat = thrown_right(900.0, 600.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    assert_eq!(cat.x, SCREEN.max_x);
    assert_eq!(cat.facing, Facing::Right);
    let height = cat.y;
    assert!(height > CLING_MIN_HEIGHT);
    assert!(matches!(cat.look(IDLE).pose, Pose::Cling(_)));

    cat.update(0.5, SCREEN, IDLE, &mut Rng::new(1));
    assert_eq!((cat.x, cat.y), (SCREEN.max_x, height), "holds on");
}

#[test]
fn clinging_cat_jumps_off_away_from_the_wall_and_lands() {
    let mut cat = thrown_right(900.0, 600.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    cat.cling_slides = false;
    assert!(run_until(&mut cat, Activity::Falling, 3.0));
    assert_eq!(cat.facing, Facing::Left, "turns to jump away");
    assert!(run_until(&mut cat, Activity::Idle, 3.0));
    assert_eq!(cat.y, 0.0);
    assert!(cat.x < SCREEN.max_x, "landed off the wall at x = {}", cat.x);
}

#[test]
fn hitting_the_wall_near_the_ground_just_lands() {
    let mut cat = thrown_right(900.0, CLING_MIN_HEIGHT / 2.0);
    assert!(!run_until(&mut cat, Activity::Clinging, 2.0));
    assert_eq!(cat.y, 0.0);
}

#[test]
fn clinging_paws_scrabble_then_settle() {
    let mut cat = thrown_right(900.0, 600.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    let mut phases = std::collections::HashSet::new();
    for _ in 0..12 {
        cat.update(SCRABBLE_SECONDS / 12.0, SCREEN, IDLE, &mut Rng::new(1));
        if let Pose::Cling(phase) = cat.look(IDLE).pose {
            phases.insert(phase);
        }
    }
    assert!(phases.len() > 1, "paws move at first: {phases:?}");
    cat.update(0.2, SCREEN, IDLE, &mut Rng::new(1));
    assert_eq!(cat.look(IDLE).pose, Pose::Cling(1));
}

#[test]
fn clinging_cat_has_no_bubble() {
    let mut cat = thrown_right(900.0, 600.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    assert_eq!(cat.look(Inputs { busy: true, ..IDLE }).bubble, Bubble::None);
}

/// A cat flying straight up at `speed`, released at `height`.
fn thrown_up(speed: f64, height: f64) -> Cat {
    let mut cat = held_cat_at(500.0, height);
    for _ in 0..10 {
        cat.drag_to(500.0, cat.y + speed / 60.0);
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    }
    cat.release();
    cat
}

#[test]
fn hard_upward_throw_sticks_the_cat_to_the_top_upside_down() {
    let mut cat = thrown_up(1200.0, 400.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    assert_eq!(cat.y, SCREEN.max_y);
    assert!(matches!(cat.look(IDLE).pose, Pose::Ceiling(_)));
    cat.update(0.5, SCREEN, IDLE, &mut Rng::new(1));
    assert_eq!(cat.y, SCREEN.max_y, "holds on");
}

#[test]
fn ceiling_cat_drops_straight_down_and_lands() {
    let mut cat = thrown_up(1200.0, 400.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    let x = cat.x;
    assert!(run_until(&mut cat, Activity::Falling, 3.0));
    assert!(run_until(&mut cat, Activity::Idle, 3.0));
    assert_eq!((cat.x, cat.y), (x, 0.0));
}

#[test]
fn gentle_upward_throw_just_bumps_the_top() {
    let mut cat = thrown_up(1200.0, 400.0);
    cat.vy = CLING_MIN_SPEED / 2.0;
    cat.y = SCREEN.max_y - 1.0;
    cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    assert_ne!(cat.activity(), Activity::Clinging);
}

/// The screen with another screen past its right edge and one above it.
const BETWEEN_SCREENS: Bounds = Bounds {
    open: OpenEdges {
        left: false,
        right: true,
        top: true,
    },
    ..SCREEN
};

/// Runs a thrown cat for `seconds` between screens, checking it never clings,
/// and returns the furthest right and highest it flew.
fn fly_between_screens(cat: &mut Cat, seconds: f64) -> (f64, f64) {
    let mut rng = Rng::new(1);
    let (mut right, mut high) = (cat.x, cat.y);
    for _ in 0..(seconds * 240.0) as usize {
        cat.update(1.0 / 240.0, BETWEEN_SCREENS, IDLE, &mut rng);
        assert_ne!(cat.activity(), Activity::Clinging);
        right = right.max(cat.x);
        high = high.max(cat.y);
    }
    (right, high)
}

#[test]
fn hard_throw_flies_on_to_the_next_screen() {
    let mut cat = thrown_right(900.0, 600.0);
    let (right, _) = fly_between_screens(&mut cat, 1.0);
    assert!(right > SCREEN.max_x + CAT_WIDTH, "flew to x = {right}");
}

#[test]
fn upward_throw_flies_on_to_the_screen_above() {
    let mut cat = thrown_up(1200.0, 400.0);
    let (_, high) = fly_between_screens(&mut cat, 0.5);
    assert!(high > SCREEN.max_y, "flew to y = {high}");
}

#[test]
fn edges_without_a_screen_past_them_stay_walls() {
    let mut cat = held_cat_at(300.0, 600.0);
    for _ in 0..10 {
        cat.drag_to(cat.x - 900.0 / 60.0, 600.0);
        cat.update(1.0 / 60.0, BETWEEN_SCREENS, IDLE, &mut Rng::new(1));
    }
    cat.release();
    let mut rng = Rng::new(1);
    while cat.activity() == Activity::Falling {
        cat.update(1.0 / 240.0, BETWEEN_SCREENS, IDLE, &mut rng);
    }
    assert_eq!(cat.activity(), Activity::Clinging);
    assert_eq!(cat.x, SCREEN.min_x);
}

#[test]
fn cat_thrown_up_from_the_screen_below_rises_before_it_lands() {
    let mut cat = thrown_up(1200.0, 400.0);
    // It just came through the bottom of this screen, still going up.
    cat.y = -5.0;
    cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    assert_eq!(cat.activity(), Activity::Falling);
    assert!(cat.y > 0.0, "still rising at y = {}", cat.y);
    assert!(run_until(&mut cat, Activity::Idle, 3.0));
    assert_eq!(cat.y, 0.0);
}

#[test]
fn held_cat_moved_to_another_screens_ground_is_not_thrown() {
    let mut cat = held_cat_at(300.0, 400.0);
    cat.raise_ground(150.0);
    assert_eq!(cat.y, 250.0);
    cat.update(1.0 / 60.0, SCREEN, IDLE, &mut Rng::new(1));
    cat.release();
    assert!(cat.vy.abs() < 1.0, "thrown at vy = {}", cat.vy);
}

#[test]
fn held_cat_can_be_carried_to_the_next_screen() {
    let mut cat = Cat::new(100.0);
    cat.grab();
    cat.drag_to(1050.0, 5000.0);
    cat.update(0.016, BETWEEN_SCREENS, IDLE, &mut Rng::new(1));
    assert_eq!((cat.x, cat.y), (1050.0, 5000.0));
}

#[test]
fn stroking_the_cat_makes_it_happy_with_hearts() {
    let mut cat = Cat::new(300.0);
    cat.stroke(PET_THRESHOLD / 2.0);
    assert!(!cat.is_petted(), "a brush past is not petting");
    cat.stroke(PET_THRESHOLD / 2.0);
    assert!(cat.is_petted());
    let look = cat.look(IDLE);
    assert_eq!(look.pose, Pose::Blink);
    assert!(matches!(look.bubble, Bubble::Hearts(_)));

    let x = cat.x;
    cat.update(1.0, SCREEN, IDLE, &mut Rng::new(1));
    assert_eq!(cat.x, x, "a petted cat stays put");
}

#[test]
fn happiness_fades_after_stroking_stops() {
    let mut cat = Cat::new(300.0);
    cat.stroke(PET_THRESHOLD);
    cat.update(PET_LINGER + 0.1, SCREEN, IDLE, &mut Rng::new(1));
    assert!(!cat.is_petted());
    assert!(!matches!(cat.look(IDLE).bubble, Bubble::Hearts(_)));
}

#[test]
fn slow_strokes_decay_before_adding_up() {
    let mut cat = Cat::new(300.0);
    for _ in 0..10 {
        cat.stroke(PET_THRESHOLD / 4.0);
        cat.update(1.0, SCREEN, IDLE, &mut Rng::new(1));
    }
    assert!(!cat.is_petted());
}

#[test]
fn a_cat_in_the_air_cannot_be_petted() {
    let mut cat = held_cat_at(300.0, 400.0);
    cat.stroke(PET_THRESHOLD * 2.0);
    assert!(!cat.is_petted());
}

#[test]
fn cat_sometimes_slides_down_the_wall_instead_of_jumping() {
    let mut cat = thrown_right(900.0, 600.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    cat.cling_slides = true;
    let height = cat.y;
    let mut rng = Rng::new(1);
    let mut lowest = height;
    let mut scrabbling = std::collections::HashSet::new();
    while cat.activity() == Activity::Clinging {
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut rng);
        assert_eq!(cat.x, SCREEN.max_x, "stays against the wall");
        assert!(cat.y <= lowest, "only ever goes down");
        lowest = cat.y;
        if cat.y < height - 50.0
            && let Pose::Cling(phase) = cat.look(IDLE).pose
        {
            scrabbling.insert(phase);
        }
    }
    assert_eq!(cat.activity(), Activity::Idle, "lands at the bottom");
    assert_eq!(cat.y, 0.0);
    assert_eq!(cat.facing, Facing::Left, "turns away from the wall");
    assert!(scrabbling.len() > 1, "claws scrape on the way down");
}

#[test]
fn slides_happen_now_and_then() {
    let mut slides = 0;
    let mut rng = Rng::new(7);
    for _ in 0..200 {
        let mut cat = Cat::new(500.0);
        cat.y = 500.0;
        cat.vx = 900.0;
        cat.x = SCREEN.max_x;
        cat.activity = Activity::Falling;
        cat.update(1.0 / 60.0, SCREEN, IDLE, &mut rng);
        assert_eq!(cat.activity(), Activity::Clinging);
        slides += usize::from(cat.cling_slides);
    }
    let share = slides as f64 / 200.0;
    assert!(
        (share - SLIDE_CHANCE).abs() < 0.12,
        "slid {share} of the time"
    );
}

#[test]
fn ceiling_cat_never_slides() {
    let mut cat = thrown_up(1200.0, 400.0);
    assert!(run_until(&mut cat, Activity::Clinging, 1.0));
    cat.cling_slides = true;
    assert!(run_until(&mut cat, Activity::Falling, 3.0), "drops instead");
}
