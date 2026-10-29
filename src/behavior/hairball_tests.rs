use super::hairball::{
    GOBBLE_SECONDS, HACK_SECONDS, LINGER_SECONDS, SNIFF_SECONDS, SPARKLE_SECONDS, SPIT_SECONDS,
    WIPE_DISTANCE, WIPE_PER_TAP,
};
use super::*;
use crate::sprites::{Bubble, Prop, SPIT};

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

fn idle_cat(x: f64) -> Cat {
    let mut cat = Cat::new(x);
    cat.activity = Activity::Idle;
    cat.time_left = 1000.0;
    cat
}

fn run(cat: &mut Cat, seconds: f64) {
    let mut rng = Rng::new(1);
    for _ in 0..(seconds / TICK).ceil() as usize {
        cat.update(TICK, SCREEN, IDLE, &mut rng);
    }
}

/// A cat that just left a hairball on the floor, and stays put.
fn cat_with_hairball(x: f64) -> Cat {
    let mut cat = idle_cat(x);
    assert!(cat.cough_up());
    run(&mut cat, HACK_SECONDS + TICK);
    cat.time_left = 1000.0;
    cat
}

fn hairball(cat: &Cat) -> Option<Prop> {
    cat.hairball().map(|view| view.prop)
}

#[test]
fn coughing_cat_heaves_then_spits() {
    let mut cat = idle_cat(400.0);
    assert!(cat.cough_up());
    assert_eq!(cat.activity(), Activity::Hacking);
    let mut poses = Vec::new();
    for _ in 0..((HACK_SECONDS - SPIT_SECONDS) / TICK) as usize - 1 {
        run(&mut cat, TICK);
        poses.push(cat.look(IDLE).pose);
    }
    assert!(poses.contains(&Pose::Hack(0)) && poses.contains(&Pose::Hack(1)));
    assert_eq!(hairball(&cat), None, "nothing comes up before the end");
    run(&mut cat, SPIT_SECONDS / 2.0);
    assert_eq!(cat.look(IDLE).pose, Pose::Hack(SPIT));
}

#[test]
fn hairball_lands_in_front_of_the_cat_and_stays_a_while() {
    let cat = cat_with_hairball(400.0);
    assert_eq!(cat.activity(), Activity::Idle);
    let view = cat.hairball().unwrap();
    assert_eq!(view.prop, Prop::Hairball(3));
    assert!(view.x as f64 > 400.0 + CAT_WIDTH / 2.0, "past the head");

    let mut walked_off = cat_with_hairball(400.0);
    walked_off.time_left = 0.0;
    run(&mut walked_off, LINGER_SECONDS - 1.0);
    assert_eq!(walked_off.hairball(), Some(view), "left where it was");
}

#[test]
fn hairball_left_alone_goes_away_after_ten_seconds() {
    let mut cat = cat_with_hairball(400.0);
    run(&mut cat, LINGER_SECONDS - 0.5);
    assert!(cat.hairball().is_some());
    run(&mut cat, 0.5);
    assert_eq!(cat.hairball(), None);
    assert!(cat.cough_up(), "room for another");
}

#[test]
fn hairball_lands_on_the_screen_when_facing_its_edge() {
    let mut cat = idle_cat(SCREEN.max_x);
    assert!(cat.cough_up());
    run(&mut cat, HACK_SECONDS + TICK);
    let x = cat.hairball().unwrap().x as f64;
    assert!(x + PROP_WIDTH <= SCREEN.max_x + CAT_WIDTH + 0.5);

    let mut cat = idle_cat(SCREEN.min_x);
    cat.facing = Facing::Left;
    assert!(cat.cough_up());
    run(&mut cat, HACK_SECONDS + TICK);
    assert!(cat.hairball().unwrap().x as f64 >= SCREEN.min_x);
}

#[test]
fn only_one_hairball_at_a_time() {
    let mut cat = cat_with_hairball(400.0);
    assert!(!cat.cough_up());
}

#[test]
fn busy_cat_does_not_cough() {
    let mut cat = idle_cat(400.0);
    assert!(cat.feed(SCREEN));
    assert!(!cat.cough_up());
    let mut held = idle_cat(400.0);
    held.grab();
    assert!(!held.cough_up());
}

#[test]
fn scrubbing_wipes_the_hairball_away_bit_by_bit() {
    let mut cat = cat_with_hairball(400.0);
    cat.wipe_hairball(WIPE_DISTANCE * 0.4);
    assert_eq!(hairball(&cat), Some(Prop::Hairball(2)));
    cat.wipe_hairball(-WIPE_DISTANCE * 0.3);
    assert_eq!(hairball(&cat), Some(Prop::Hairball(1)));
    cat.wipe_hairball(WIPE_DISTANCE * 0.3);
    assert!(matches!(hairball(&cat), Some(Prop::Sparkle(_))));
    run(&mut cat, SPARKLE_SECONDS);
    assert_eq!(hairball(&cat), None);
    assert!(cat.cough_up(), "the floor is clean again");
}

#[test]
fn clicks_wipe_the_hairball_too() {
    let mut cat = cat_with_hairball(400.0);
    let taps = (WIPE_DISTANCE / WIPE_PER_TAP).ceil() as usize;
    for _ in 0..taps - 1 {
        cat.tap_hairball();
    }
    assert!(matches!(hairball(&cat), Some(Prop::Hairball(_))));
    cat.tap_hairball();
    assert!(matches!(hairball(&cat), Some(Prop::Sparkle(_))));
}

#[test]
fn opening_the_chat_interrupts_the_cough() {
    let mut cat = idle_cat(400.0);
    assert!(cat.cough_up());
    let chat = Inputs {
        chat_open: true,
        ..IDLE
    };
    cat.update(TICK, SCREEN, chat, &mut Rng::new(1));
    assert_eq!(cat.activity(), Activity::Idle);
    run(&mut cat, HACK_SECONDS + TICK);
    assert_eq!(cat.hairball(), None);
}

#[test]
fn idle_cat_coughs_up_a_hairball_now_and_then() {
    let mut rng = Rng::new(7);
    let coughs = (0..2000)
        .filter(|_| {
            let mut cat = idle_cat(400.0);
            cat.time_left = 0.0;
            cat.update(TICK, SCREEN, IDLE, &mut rng);
            cat.activity() == Activity::Hacking
        })
        .count();
    let expected = 2000.0 * HAIRBALL_CHANCE;
    assert!(
        (coughs as f64 - expected).abs() < expected / 2.0,
        "{coughs}"
    );
}

/// Dédé, idle and staying put.
fn idle_pig(x: f64) -> Cat {
    let mut pig = idle_cat(x);
    pig.set_pig(true);
    pig
}

#[test]
fn dede_squats_poops_behind_him_and_turns_around() {
    let mut pig = idle_pig(400.0);
    assert!(pig.cough_up());
    run(&mut pig, TICK);
    assert!(matches!(pig.look(IDLE).pose, Pose::Squat(_)));
    run(&mut pig, HACK_SECONDS);
    assert_eq!(hairball(&pig), Some(Prop::Poop(3)));
    let poop = pig.hairball().unwrap();
    assert!(poop.x < 400, "behind a pig facing right");
    assert_eq!(pig.facing, Facing::Left, "turned around to it");
    assert_eq!(pig.activity(), Activity::Gobbling);
    assert!(matches!(pig.look(IDLE).bubble, Bubble::Hearts(_)));
}

#[test]
fn dede_eats_his_poop_and_leaves_nothing() {
    let mut pig = idle_pig(400.0);
    pig.cough_up();
    run(
        &mut pig,
        HACK_SECONDS + SNIFF_SECONDS + GOBBLE_SECONDS / 2.0,
    );
    assert!(matches!(pig.look(IDLE).pose, Pose::Eat(_)));
    assert!(matches!(hairball(&pig), Some(Prop::Poop(level)) if level < 3));
    run(&mut pig, GOBBLE_SECONDS / 2.0 + 0.1);
    assert_eq!(hairball(&pig), None, "eaten, no sparkle");
    assert_eq!(pig.activity(), Activity::Idle);
}

#[test]
fn wiping_dedes_poop_first_spoils_his_meal() {
    let mut pig = idle_pig(400.0);
    pig.cough_up();
    run(&mut pig, HACK_SECONDS + TICK);
    pig.wipe_hairball(WIPE_DISTANCE);
    run(&mut pig, TICK);
    assert_eq!(pig.activity(), Activity::Idle);
    assert!(matches!(hairball(&pig), Some(Prop::Sparkle(_))));
}

#[test]
fn a_cat_still_coughs_up_hairballs() {
    let cat = cat_with_hairball(400.0);
    assert_eq!(hairball(&cat), Some(Prop::Hairball(3)));
    assert_eq!(cat.activity(), Activity::Idle);
}
