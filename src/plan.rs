//! The day plan: a plan quick action (Today's planning) reports it in a fenced
//! `meow-plan` JSON block, and it is shown as its own page (agenda, focus,
//! replies) instead of prose. Its items join the task list.

use serde::{Deserialize, Deserializer, Serialize};

use crate::clock;
use crate::tasks::{
    FoundTask, NOT_MY_TASK_RULE, ONE_TASK_PER_REQUEST_RULE, SLACK_NO_REPLY_RULE, take_fence,
};

pub const PLAN_FENCE: &str = "meow-plan";
const MAX_EVENTS: usize = 20;
const MAX_ITEMS: usize = 12;
const MAX_TEXT_CHARS: usize = 300;
/// An event without an end is taken to last this long.
const DEFAULT_EVENT_MINUTES: u32 = 30;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayPlan {
    /// One or two sentences on the day.
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub agenda: Vec<Event>,
    /// What to tackle first, in order.
    #[serde(default)]
    pub focus: Vec<PlanItem>,
    /// Messages waiting for an answer.
    #[serde(default)]
    pub replies: Vec<PlanItem>,
    /// Everything else to act on, when there is time.
    #[serde(default)]
    pub later: Vec<PlanItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// "09:30"; `None` for an all-day event.
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    /// The calendar's id of the event, to answer the invitation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The calendar holding it, when not the primary one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<String>,
    /// Where it happens: the meeting room or place (not a video link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// My answer to the invitation; `None` when there is nothing to answer
    /// (I organize it, or I am not invited).
    #[serde(
        default,
        deserialize_with = "lenient_rsvp",
        skip_serializing_if = "Option::is_none"
    )]
    pub response: Option<Rsvp>,
}

/// An answer to an invitation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Rsvp {
    NeedsAction,
    Accepted,
    Tentative,
    Declined,
}

impl Rsvp {
    /// "accepted", "needsAction"… as calendars spell them; anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        match text
            .trim()
            .to_ascii_lowercase()
            .replace(['_', ' '], "")
            .as_str()
        {
            "needsaction" | "pending" | "invited" => Some(Self::NeedsAction),
            "accepted" | "yes" => Some(Self::Accepted),
            "tentative" | "maybe" => Some(Self::Tentative),
            "declined" | "no" => Some(Self::Declined),
            _ => None,
        }
    }

    /// The word calendars use, for prompts.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeedsAction => "needsAction",
            Self::Accepted => "accepted",
            Self::Tentative => "tentative",
            Self::Declined => "declined",
        }
    }
}

/// An unknown or malformed answer reads as no answer, never as a broken plan.
fn lenient_rsvp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Rsvp>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .and_then(Rsvp::parse))
}

/// How Claude reports one agenda event, shared by the plan and its refresh.
pub const EVENT_SHAPE: &str = "{\"start\": \"HH:MM\" (24-hour, my local time; null if \
     all-day), \"end\": \"HH:MM\" (give it whenever the event has one) or null, \"title\": short title, \"url\": link or null, \
     \"location\": the meeting room or place, \
     null when there is none or it is only a video link, \"id\": the calendar's event id, \"calendar\": the calendar id when not my primary \
     calendar, else null, \"response\": my answer to the invitation, \"accepted\", \
     \"declined\", \"tentative\" or \"needsAction\", or null when I organize it or am not \
     invited}";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    pub title: String,
    /// Why it matters today, for focus items.
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

impl PlanItem {
    pub fn task(&self) -> FoundTask {
        FoundTask {
            title: self.title.clone(),
            source: self.source.clone(),
            url: self.url.clone(),
        }
    }
}

/// Where an event stands compared to now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventState {
    AllDay,
    Past,
    Now,
    Upcoming,
}

impl Event {
    pub fn state(&self, now_minutes: u32) -> EventState {
        let Some(start) = self.start.as_deref().and_then(minutes) else {
            return EventState::AllDay;
        };
        let end = self
            .end
            .as_deref()
            .and_then(minutes)
            .filter(|end| *end > start)
            .unwrap_or(start + DEFAULT_EVENT_MINUTES);
        if now_minutes >= end {
            EventState::Past
        } else if now_minutes >= start {
            EventState::Now
        } else {
            EventState::Upcoming
        }
    }

    /// How long it lasts, in minutes, when it has a start and a later end.
    pub fn duration_minutes(&self) -> Option<u32> {
        let start = self.start.as_deref().and_then(minutes)?;
        let end = self.end.as_deref().and_then(minutes)?;
        (end > start).then(|| end - start)
    }

    /// "30 min", "1 h", "1 h 30".
    pub fn duration_label(&self) -> Option<String> {
        self.duration_minutes()
            .map(|total| match (total / 60, total % 60) {
                (0, m) => format!("{m} min"),
                (h, 0) => format!("{h} h"),
                (h, m) => format!("{h} h {m:02}"),
            })
    }

    /// Whether the invitation can be answered from the plan.
    pub fn answerable(&self) -> bool {
        self.id.is_some() && self.response.is_some()
    }

    fn cleaned(self) -> Option<Self> {
        if self.calendar.as_deref().is_some_and(is_room_calendar) {
            return None;
        }
        let id = self.id.as_deref().and_then(clean_text);
        Some(Self {
            title: clean_text(&self.title)?,
            start: self.start.as_deref().and_then(normalize_time),
            end: self.end.as_deref().and_then(normalize_time),
            url: self.url.as_deref().and_then(clean_text),
            calendar: self.calendar.as_deref().and_then(clean_text),
            location: self
                .location
                .as_deref()
                .and_then(clean_text)
                .filter(|place| !is_link(place)),
            // Without an id there is no answering.
            response: self.response.filter(|_| id.is_some()),
            id,
        })
    }
}

impl DayPlan {
    /// The item lists with their headings, in display order.
    pub fn sections(&self) -> [(&'static str, &[PlanItem]); 3] {
        [
            ("Focus", &self.focus),
            ("Replies", &self.replies),
            ("Later", &self.later),
        ]
    }

    /// Every item, as tasks for the task list.
    pub fn tasks(&self) -> Vec<FoundTask> {
        self.sections()
            .iter()
            .flat_map(|(_, items)| items.iter().map(PlanItem::task))
            .collect()
    }

    /// The same plan with a fresh agenda, e.g. after the calendar changed.
    pub fn with_agenda(&self, agenda: Vec<Event>) -> Self {
        Self {
            agenda: clean_agenda(agenda),
            ..self.clone()
        }
    }

    /// The same plan with my answer to event `id` recorded.
    pub fn with_response(&self, id: &str, response: Rsvp) -> Self {
        let agenda = self
            .agenda
            .iter()
            .map(|event| match event.id.as_deref() {
                Some(event_id) if event_id == id => Event {
                    response: Some(response),
                    ..event.clone()
                },
                _ => event.clone(),
            })
            .collect();
        Self {
            agenda,
            ..self.clone()
        }
    }

    /// Trims and bounds what Claude sent, sorts the agenda; `None` if nothing is left.
    fn cleaned(self) -> Option<Self> {
        let plan = Self {
            summary: clean_text(&self.summary).unwrap_or_default(),
            agenda: clean_agenda(self.agenda),
            focus: clean_items(self.focus),
            replies: clean_items(self.replies),
            later: clean_items(self.later),
        };
        let empty = plan.summary.is_empty()
            && plan.agenda.is_empty()
            && plan.sections().iter().all(|(_, items)| items.is_empty());
        (!empty).then_some(plan)
    }
}

/// Trims and bounds the events Claude sent, sorted by start time.
pub fn clean_agenda(events: Vec<Event>) -> Vec<Event> {
    let mut agenda: Vec<Event> = events
        .into_iter()
        .filter_map(Event::cleaned)
        .take(MAX_EVENTS)
        .collect();
    agenda.sort_by_key(|event| event.start.as_deref().and_then(minutes));
    agenda
}

fn clean_items(items: Vec<PlanItem>) -> Vec<PlanItem> {
    items
        .into_iter()
        .filter_map(|item| {
            Some(PlanItem {
                title: clean_text(&item.title)?,
                why: item.why.as_deref().and_then(clean_text),
                source: item.source.as_deref().and_then(clean_text),
                url: item.url.as_deref().and_then(clean_text),
            })
        })
        .take(MAX_ITEMS)
        .collect()
}

fn clean_text(text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| text.chars().take(MAX_TEXT_CHARS).collect())
}

/// A meeting room's calendar: its events are the room's bookings, not mine.
fn is_room_calendar(calendar: &str) -> bool {
    calendar
        .trim()
        .to_ascii_lowercase()
        .ends_with("@resource.calendar.google.com")
}

/// A video or web link, which is no meeting room.
fn is_link(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.contains("meet.google.com")
}

/// Minutes since midnight of "9:30" or "09:30".
fn minutes(time: &str) -> Option<u32> {
    let (hour, minute) = time.trim().split_once(':')?;
    let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

/// "9:30" → "09:30"; anything that is not a time of day → `None`.
fn normalize_time(time: &str) -> Option<String> {
    minutes(time).map(|m| format!("{:02}:{:02}", m / 60, m % 60))
}

/// Splits Claude's answer into the text to show and the plan it reported.
pub fn extract(answer: &str) -> (String, Option<DayPlan>) {
    let (shown, body) = take_fence(answer, PLAN_FENCE);
    let plan = body
        .and_then(|body| serde_json::from_str::<DayPlan>(&body).ok())
        .and_then(DayPlan::cleaned);
    (shown, plan)
}

/// Asks Claude to report its plan as a `meow-plan` block, for the plan page.
pub fn instruction(now: u64) -> String {
    format!(
        "{} Your answer is shown as a structured page, so keep the prose to one or two \
         sentences summing up the day, then end with a fenced code block with the info string \
         `{PLAN_FENCE}` holding one JSON object:\n\
         {{\"summary\": the same one or two sentences,\n \
         \"agenda\": today's events on my primary calendar in order, declined ones included, \
         never the bookings of meeting rooms or other calendars I only subscribe to, each \
         {EVENT_SHAPE},\n \
         \"focus\": the 3 things to tackle first, most important first, each {{\"title\": short \
         imperative title, \"why\": why it matters today in a few words, \"source\": Linear, \
         GitHub, Slack, Gmail…, \"url\": link or null}},\n \
         \"replies\": messages waiting for my answer, same shape without \"why\",\n \
         \"later\": anything else I need to act on, same shape}}.\n\
         Do not repeat an item in two lists, and use empty lists when there is nothing. \
         {SLACK_NO_REPLY_RULE} {NOT_MY_TASK_RULE} {ONE_TASK_PER_REQUEST_RULE}",
        clock::describe(now)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(start: Option<&str>, end: Option<&str>) -> Event {
        Event {
            start: start.map(Into::into),
            end: end.map(Into::into),
            title: "Standup".into(),
            url: None,
            id: None,
            calendar: None,
            location: None,
            response: None,
        }
    }

    #[test]
    fn extract_strips_the_block_and_parses_the_plan() {
        let answer = "Busy day.\n\n```meow-plan\n{\"summary\": \"Busy day.\", \
            \"agenda\": [{\"start\": \"14:00\", \"title\": \"1:1\"}, \
                         {\"start\": \"9:30\", \"end\": \"9:45\", \"title\": \"Standup\"}], \
            \"focus\": [{\"title\": \"Fix ACC-52\", \"why\": \"blocks release\", \
                         \"source\": \"Linear\", \"url\": \"https://l/52\"}]}\n```";
        let (shown, plan) = extract(answer);
        let plan = plan.unwrap();
        assert_eq!(shown, "Busy day.");
        let starts: Vec<_> = plan.agenda.iter().map(|e| e.start.as_deref()).collect();
        assert_eq!(starts, [Some("09:30"), Some("14:00")]);
        assert_eq!(plan.focus[0].why.as_deref(), Some("blocks release"));
        assert!(plan.replies.is_empty() && plan.later.is_empty());
    }

    #[test]
    fn extract_tolerates_missing_or_broken_plans() {
        assert_eq!(extract("hi"), ("hi".into(), None));
        assert_eq!(extract("hi\n```meow-plan\nnope\n```").1, None);
        assert_eq!(extract("```meow-plan\n{}\n```").1, None, "empty plan");
    }

    #[test]
    fn cleaning_drops_untitled_items_and_bad_times() {
        let plan = DayPlan {
            agenda: vec![
                Event {
                    start: Some("lunch".into()),
                    ..event(None, None)
                },
                Event {
                    title: " ".into(),
                    ..event(Some("10:00"), None)
                },
            ],
            later: vec![
                PlanItem {
                    title: "  Reply  to Bo ".into(),
                    why: Some(" ".into()),
                    source: None,
                    url: Some("".into()),
                },
                PlanItem {
                    title: "".into(),
                    why: None,
                    source: None,
                    url: None,
                },
            ],
            ..DayPlan::default()
        }
        .cleaned()
        .unwrap();
        assert_eq!(plan.agenda.len(), 1);
        assert_eq!(
            plan.agenda[0].start, None,
            "unreadable time reads as all-day"
        );
        assert_eq!(plan.later.len(), 1);
        let item = &plan.later[0];
        assert_eq!(
            (item.title.as_str(), &item.why, &item.url),
            ("Reply to Bo", &None, &None)
        );
    }

    #[test]
    fn cleaning_drops_meeting_room_bookings() {
        let room = Event {
            calendar: Some("c_1886ea63@resource.calendar.google.com".into()),
            ..event(Some("09:00"), None)
        };
        let shared = Event {
            calendar: Some("team@group.calendar.google.com".into()),
            ..event(Some("10:00"), None)
        };
        let agenda = clean_agenda(vec![room, shared, event(Some("11:00"), None)]);
        let starts: Vec<_> = agenda.iter().map(|e| e.start.as_deref()).collect();
        assert_eq!(starts, [Some("10:00"), Some("11:00")]);
    }

    #[test]
    fn events_know_whether_they_are_past_now_or_upcoming() {
        let ten = 10 * 60;
        let standup = event(Some("09:30"), Some("10:00"));
        assert_eq!(standup.state(9 * 60), EventState::Upcoming);
        assert_eq!(standup.state(9 * 60 + 45), EventState::Now);
        assert_eq!(standup.state(ten), EventState::Past);
        let no_end = event(Some("10:00"), None);
        assert_eq!(no_end.state(ten + 29), EventState::Now);
        assert_eq!(no_end.state(ten + 30), EventState::Past);
        assert_eq!(event(None, None).state(ten), EventState::AllDay);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(
            event(Some("09:30"), Some("10:00"))
                .duration_label()
                .as_deref(),
            Some("30 min")
        );
        assert_eq!(
            event(Some("14:00"), Some("15:00"))
                .duration_label()
                .as_deref(),
            Some("1 h")
        );
        assert_eq!(
            event(Some("9:00"), Some("10:30"))
                .duration_label()
                .as_deref(),
            Some("1 h 30")
        );
        assert_eq!(event(Some("10:00"), None).duration_label(), None);
        assert_eq!(event(Some("10:00"), Some("09:00")).duration_label(), None);
        assert_eq!(event(None, None).duration_label(), None);
    }

    #[test]
    fn rooms_are_kept_but_video_links_are_not() {
        let text = r#"{"agenda": [
            {"start": "9:00", "title": "A", "location": " Salle  Kodak "},
            {"start": "10:00", "title": "B", "location": "https://meet.google.com/abc"}]}"#;
        let plan = serde_json::from_str::<DayPlan>(text)
            .unwrap()
            .cleaned()
            .unwrap();
        let rooms: Vec<_> = plan.agenda.iter().map(|e| e.location.as_deref()).collect();
        assert_eq!(rooms, [Some("Salle Kodak"), None]);
    }

    #[test]
    fn invitations_are_answerable_only_with_an_id() {
        let text = r#"{"agenda": [
            {"start": "9:00", "title": "A", "id": "e1", "response": "needsAction"},
            {"start": "10:00", "title": "B", "response": "accepted"},
            {"start": "11:00", "title": "C", "id": "e3", "response": "whatever"},
            {"start": "12:00", "title": "D", "id": "e4", "response": 3}]}"#;
        let plan = serde_json::from_str::<DayPlan>(text)
            .unwrap()
            .cleaned()
            .unwrap();
        let answers: Vec<_> = plan.agenda.iter().map(|e| e.response).collect();
        assert_eq!(answers, [Some(Rsvp::NeedsAction), None, None, None]);
        assert!(plan.agenda[0].answerable() && !plan.agenda[1].answerable());
    }

    #[test]
    fn an_answer_updates_only_its_event() {
        let invited = |id: &str| Event {
            id: Some(id.into()),
            response: Some(Rsvp::NeedsAction),
            ..event(Some("09:00"), None)
        };
        let plan = DayPlan {
            agenda: vec![invited("a"), invited("b")],
            ..DayPlan::default()
        };
        let updated = plan.with_response("b", Rsvp::Declined);
        assert_eq!(updated.agenda[0].response, Some(Rsvp::NeedsAction));
        assert_eq!(updated.agenda[1].response, Some(Rsvp::Declined));
        let saved = serde_json::to_string(&updated.agenda[1]).unwrap();
        assert!(saved.contains("\"response\":\"declined\""), "{saved}");
    }

    #[test]
    fn a_fresh_agenda_replaces_only_the_events() {
        let plan = DayPlan {
            summary: "Busy day.".into(),
            agenda: vec![event(Some("09:00"), None)],
            ..DayPlan::default()
        };
        let moved = Event {
            title: "Moved".into(),
            ..event(Some("8:15"), None)
        };
        let updated = plan.with_agenda(vec![event(Some("11:00"), None), moved]);
        let starts: Vec<_> = updated.agenda.iter().map(|e| e.start.as_deref()).collect();
        assert_eq!(starts, [Some("08:15"), Some("11:00")]);
        assert_eq!(updated.summary, "Busy day.");
        assert!(plan.with_agenda(Vec::new()).agenda.is_empty());
    }

    #[test]
    fn every_item_becomes_a_task() {
        let item = |title: &str| PlanItem {
            title: title.into(),
            why: None,
            source: Some("Slack".into()),
            url: None,
        };
        let plan = DayPlan {
            focus: vec![item("a")],
            replies: vec![item("b")],
            later: vec![item("c")],
            ..DayPlan::default()
        };
        let titles: Vec<_> = plan.tasks().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["a", "b", "c"]);
    }

    #[test]
    fn instruction_names_the_fence_and_the_date() {
        let text = instruction(1_792_411_200);
        assert!(text.contains(PLAN_FENCE));
        assert!(text.starts_with("Today is "));
        assert!(text.contains(SLACK_NO_REPLY_RULE));
    }
}
