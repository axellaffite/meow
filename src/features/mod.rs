//! Every feature of the app, one folder each (see ARCHITECTURE.md). Adding a
//! feature adds its folder plus one `mod` line and one entry in `all`, kept in
//! alphabetical order so parallel branches rarely touch the same lines.

mod agenda;
mod appearance;
mod background_check;
mod daily_actions;
mod routines;
mod slack;
#[cfg(test)]
mod testing;
mod update;

use meow::settings::Store;

use crate::app::feature::Feature;

/// The features, in the order they are polled and their menu items shown,
/// each reading its settings from `settings`.
// One entry per line, even when rustfmt would join them: each branch then
// adds its own line, which git merges without a conflict.
#[rustfmt::skip]
pub(crate) fn all(settings: &mut Store) -> Vec<Box<dyn Feature>> {
    vec![
        Box::new(agenda::AgendaRefresh::from_env()),
        Box::new(appearance::Theme::from_settings(settings.get())),
        Box::new(background_check::BackgroundCheck::from_env()),
        Box::new(daily_actions::DailyActions::new()),
        Box::new(routines::ScheduledTasks::from_env()),
        Box::new(slack::Slack::from_settings(settings.get())),
        Box::new(update::Update::from_env()),
    ]
}
