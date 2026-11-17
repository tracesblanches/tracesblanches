//! Voice commands (V3 #66, VO5 #87, A11 #98): what the phone heard (Parakeet v3 on the
//! phone, `android/voicecmd`) classified into the closed list of the core's functions
//! ([`classify_command`], `skimap-intent`: no language model), and the answers that are
//! not already a call of the tracking session ([`VoiceAnswers`]).
//!
//! « Prochaine direction ? », « Temps restant ? » and « Ramène-moi » are answered by
//! [`crate::tracking::TrackingSession::say_next`], `say_remaining` and `back_start`;
//! « Répète » by the app (the last thing said). The others are made here from the events
//! of the voice bank (`skimap_voice::Event`), with the clips of the pack: « Où suis-je ? »
//! (on or off the route, distance and time left, from the guidance), « Heure de
//! demi-tour ? » (the countdown to the latest turnaround of the plan), « Combien de
//! batterie ? ». The replies of the command itself (« je n'ai pas compris » and the
//! questions understood, the outcome of a report, nothing to repeat, no outing) are new
//! phrases without clips yet: the Android voice reads them.

use std::path::{Path, PathBuf};

use chrono::{NaiveDateTime, Timelike};
use skimap_intent::{Intent, Lang};
use skimap_plan::plan::DayPlan;
use skimap_plan::tracker::TrackerParams;
use skimap_voice::Event;
use skimap_voice::bank::{Announcement, Pause, Segment};
use skimap_voice::clips::{self, Clip, Fixed};
use skimap_voice::pack::VoiceManifest;
use skimap_voice::quantity::Clock;

use crate::tracking::{
    ClipSegment, GuidanceView, PhaseView, SpeechPriority, SpokenView, TrackingError, epoch_ms,
};
use crate::tracking_day::PLAN_FILE;

/// The closed list of commands (`skimap_intent::Intent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VoiceCommand {
    /// « Où suis-je ? »
    WhereAmI,
    /// « Temps restant ? »
    TimeLeft,
    /// « Prochaine direction ? »
    NextDirection,
    /// « Ramène-moi »
    BringMeBack,
    /// « Heure de demi-tour ? »
    TurnaroundTime,
    /// « Signale : trace faite »
    ReportTracksSet,
    /// « Signale : piste damée »
    ReportGroomed,
    /// « Combien de batterie ? »
    Battery,
    /// « Répète »
    Repeat,
}

impl From<Intent> for VoiceCommand {
    fn from(i: Intent) -> Self {
        match i {
            Intent::WhereAmI => Self::WhereAmI,
            Intent::TimeLeft => Self::TimeLeft,
            Intent::NextDirection => Self::NextDirection,
            Intent::BringMeBack => Self::BringMeBack,
            Intent::TurnaroundTime => Self::TurnaroundTime,
            Intent::ReportTracksSet => Self::ReportTracksSet,
            Intent::ReportGroomed => Self::ReportGroomed,
            Intent::Battery => Self::Battery,
            Intent::Repeat => Self::Repeat,
        }
    }
}

/// Language of a question and of the replies of the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CommandLang {
    /// French.
    Fr,
    /// English.
    En,
}

impl From<CommandLang> for Lang {
    fn from(l: CommandLang) -> Self {
        match l {
            CommandLang::Fr => Lang::Fr,
            CommandLang::En => Lang::En,
        }
    }
}

impl From<Lang> for CommandLang {
    fn from(l: Lang) -> Self {
        match l {
            Lang::Fr => CommandLang::Fr,
            Lang::En => CommandLang::En,
        }
    }
}

/// What [`classify_command`] found.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CommandHeard {
    /// The transcription.
    pub text: String,
    /// The command, `None`: not understood.
    pub command: Option<VoiceCommand>,
    /// Language of the question.
    pub lang: CommandLang,
    /// Score of the best command (`skimap_intent::MIN_SCORE` to be chosen).
    pub score: f32,
}

/// The command asked by a transcription, `default_lang` (the phone's) when its words do
/// not tell the language.
#[uniffi::export]
pub fn classify_command(text: String, default_lang: CommandLang) -> CommandHeard {
    let c = skimap_intent::classify(&text, default_lang.into());
    CommandHeard {
        text,
        command: c.intent.map(Into::into),
        lang: c.lang.into(),
        score: c.score,
    }
}

/// The questions understood, in `lang` (shown on the screen, said after « je n'ai pas
/// compris »).
#[uniffi::export]
pub fn command_questions(lang: CommandLang) -> Vec<String> {
    skimap_intent::questions(lang.into())
        .into_iter()
        .map(String::from)
        .collect()
}

/// A community report asked by voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportKind {
    /// Classic tracks set.
    TracksSet,
    /// Trail groomed.
    Groomed,
}

/// What became of a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportOutcome {
    /// Sent, or kept to be sent once there is network.
    Recorded,
    /// No community module on this phone yet.
    Unavailable,
}

/// The answers of the voice commands said with the clips of the pack's voice layer.
#[derive(uniffi::Object)]
pub struct VoiceAnswers {
    manifest: Option<VoiceManifest>,
    dir: PathBuf,
}

impl std::fmt::Debug for VoiceAnswers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceAnswers")
            .field("dir", &self.dir)
            .field("clips", &self.manifest.is_some())
            .finish()
    }
}

/// A clip of a reply of the command (no clip in the packs yet: the Android voice).
fn reply(id: &str, text: &str) -> Clip {
    Clip {
        id: format!("cmd.{id}"),
        text: text.into(),
    }
}

fn one(clip: Clip) -> Announcement {
    Announcement {
        segments: vec![Segment {
            clip,
            pause: Pause::Short,
        }],
    }
}

/// Distance to the route beyond which « Où suis-je ? » says the skier left it: the
/// tracker's own threshold.
fn off_route_m() -> f64 {
    TrackerParams::default().enter_m
}

impl VoiceAnswers {
    /// Answers with the clips of the voice layer in `voice_dir` (`None` or unreadable: every
    /// answer is read by the Android voice).
    pub fn open(voice_dir: Option<&Path>) -> Self {
        Self {
            manifest: voice_dir.and_then(|v| VoiceManifest::read(v).ok()),
            dir: voice_dir.map(Path::to_path_buf).unwrap_or_default(),
        }
    }

    fn spoken(
        &self,
        t_ms: i64,
        a: &Announcement,
        priority: SpeechPriority,
        kind: &str,
    ) -> SpokenView {
        let n = a.segments.len();
        let mut all = true;
        let segments = a
            .segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let file = self
                    .manifest
                    .as_ref()
                    .and_then(|m| m.get(&s.clip.id))
                    .filter(|e| e.text == s.clip.text)
                    .map(|e| self.dir.join(&e.file).display().to_string());
                all &= file.is_some();
                ClipSegment {
                    id: s.clip.id.clone(),
                    text: s.clip.text.clone(),
                    file,
                    pause_ms: if i + 1 < n { s.pause.ms() } else { 0 },
                }
            })
            .collect();
        SpokenView {
            t_ms,
            text: a.text(),
            segments,
            all_clips: all,
            priority,
            kinds: vec![kind.into()],
        }
    }

    /// « Où suis-je ? » from the guidance: off the route, how far (the side is the
    /// tracker's announcement); on it, « Toujours sur l'itinéraire. »; then the distance and
    /// time left.
    pub fn where_am_i_at(&self, g: &GuidanceView, t_ms: i64) -> SpokenView {
        let remaining = Event::Remaining {
            metres: g.remaining_m,
            seconds: g.remaining_s,
        }
        .announce();
        let head = if g.phase == PhaseView::Finished {
            one(Fixed::PlanFinished.clip())
        } else if g.off_route_m.is_finite() && g.off_route_m > off_route_m() {
            let mut a = one(Fixed::OffRoute.clip());
            a.segments[0].pause = Pause::Long;
            let at = skimap_voice::quantity::round_distance(g.off_route_m);
            a.segments.push(Segment {
                clip: match at {
                    Some(m) if m <= skimap_voice::manifest::AT_MAX_M => clips::at(m),
                    _ => Fixed::RouteFar.clip(),
                },
                pause: Pause::Short,
            });
            a
        } else if g.off_route_m.is_finite() {
            Event::Reassurance { piste: None }.announce()
        } else {
            // No fix yet: only what is left of the plan.
            return self.spoken(t_ms, &remaining, SpeechPriority::Comfort, "cmd_where");
        };
        let a = if g.phase == PhaseView::Finished {
            head
        } else {
            head.then(remaining)
        };
        self.spoken(t_ms, &a, SpeechPriority::Direction, "cmd_where")
    }

    /// « Heure de demi-tour ? »: the countdown to the latest turnaround of `plan`
    /// (« Demi-tour dans vingt minutes. », « Faites demi-tour maintenant… », or the time
    /// itself), at `t_ms`.
    pub fn turnaround_at(&self, plan: &DayPlan, t_ms: i64, lang: Lang) -> SpokenView {
        match plan.turnaround.latest_turnaround {
            Some(at) => {
                let deadline = floor_quarter(at);
                let a = Event::Turnaround {
                    seconds_left: (epoch_ms(at) - t_ms) as f64 / 1000.0,
                    deadline,
                }
                .announce();
                self.spoken(t_ms, &a, SpeechPriority::Schedule, "cmd_turnaround")
            }
            None => {
                let text = match lang {
                    Lang::Fr => "Pas d'heure de demi-tour : la boucle finit avant l'heure limite.",
                    Lang::En => "No turnaround time: the loop ends before the limit.",
                };
                self.spoken(
                    t_ms,
                    &one(reply(&format!("no-turnaround.{}", code(lang)), text)),
                    SpeechPriority::Schedule,
                    "cmd_turnaround",
                )
            }
        }
    }
}

/// Never announced later than it is: rounded down to the quarter.
fn floor_quarter(t: NaiveDateTime) -> Clock {
    Clock::floor_quarter(t.hour() as u8, t.minute() as u8)
}

fn code(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "fr",
        Lang::En => "en",
    }
}

#[uniffi::export]
impl VoiceAnswers {
    /// Answers with the clips of the voice layer in `voice_dir`.
    #[uniffi::constructor]
    pub fn new(voice_dir: Option<String>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self::open(voice_dir.as_deref().map(Path::new)))
    }

    /// The voice layer has clips (a manifest was read).
    pub fn has_clips(&self) -> bool {
        self.manifest.is_some()
    }

    /// « Où suis-je ? » from the guidance of the tracking session.
    pub fn where_am_i(&self, guidance: GuidanceView, t_ms: i64) -> SpokenView {
        self.where_am_i_at(&guidance, t_ms)
    }

    /// « Heure de demi-tour ? » from the plan of the outing (`plan_dir/plan.json`).
    pub fn turnaround(
        &self,
        plan_dir: String,
        t_ms: i64,
        lang: CommandLang,
    ) -> Result<SpokenView, TrackingError> {
        let p = Path::new(&plan_dir).join(PLAN_FILE);
        let text = std::fs::read_to_string(&p).map_err(|e| TrackingError::Invalid {
            detail: format!("{}: {e}", p.display()),
        })?;
        let plan = DayPlan::from_json(&text).map_err(|e| TrackingError::Invalid {
            detail: e.to_string(),
        })?;
        Ok(self.turnaround_at(&plan, t_ms, lang.into()))
    }

    /// « Combien de batterie ? »: the level rounded down to ten percent, and the autonomy
    /// when the phone could estimate it.
    pub fn battery(&self, percent: f64, autonomy_s: Option<f64>, t_ms: i64) -> SpokenView {
        let a = match autonomy_s.filter(|s| s.is_finite() && *s > 0.0) {
            Some(s) => Event::Battery {
                percent,
                autonomy_s: s,
            }
            .announce(),
            None => {
                let p = (percent.clamp(0.0, 100.0) / 10.0).floor() as u32 * 10;
                one(clips::battery(p))
            }
        };
        self.spoken(t_ms, &a, SpeechPriority::Comfort, "cmd_battery")
    }

    /// « Je n'ai pas compris. » and the questions understood.
    pub fn not_understood(&self, lang: CommandLang, t_ms: i64) -> SpokenView {
        let l: Lang = lang.into();
        let (head, intro) = match l {
            Lang::Fr => ("Je n'ai pas compris.", "Vous pouvez demander :"),
            Lang::En => ("I did not understand.", "You can ask:"),
        };
        let c = code(l);
        let mut segments = vec![
            Segment {
                clip: reply(&format!("not-understood.{c}"), head),
                pause: Pause::Long,
            },
            Segment {
                clip: reply(&format!("you-can-ask.{c}"), intro),
                pause: Pause::Short,
            },
        ];
        for i in Intent::ALL {
            segments.push(Segment {
                clip: reply(
                    &format!("question.{}.{c}", i.name().replace('_', "-")),
                    i.question(l),
                ),
                pause: Pause::Short,
            });
        }
        self.spoken(
            t_ms,
            &Announcement { segments },
            SpeechPriority::Comfort,
            "cmd_not_understood",
        )
    }

    /// The outcome of a report asked by voice.
    pub fn report(
        &self,
        kind: ReportKind,
        outcome: ReportOutcome,
        lang: CommandLang,
        t_ms: i64,
    ) -> SpokenView {
        let l: Lang = lang.into();
        let (id, text) = match (outcome, kind, l) {
            (ReportOutcome::Recorded, ReportKind::TracksSet, Lang::Fr) => {
                ("report-tracks", "Signalement enregistré : trace faite.")
            }
            (ReportOutcome::Recorded, ReportKind::Groomed, Lang::Fr) => {
                ("report-groomed", "Signalement enregistré : piste damée.")
            }
            (ReportOutcome::Recorded, ReportKind::TracksSet, Lang::En) => {
                ("report-tracks", "Report recorded: tracks set.")
            }
            (ReportOutcome::Recorded, ReportKind::Groomed, Lang::En) => {
                ("report-groomed", "Report recorded: trail groomed.")
            }
            (ReportOutcome::Unavailable, _, Lang::Fr) => (
                "report-unavailable",
                "Les signalements ne sont pas encore disponibles sur ce téléphone.",
            ),
            (ReportOutcome::Unavailable, _, Lang::En) => (
                "report-unavailable",
                "Reports are not available on this phone yet.",
            ),
        };
        self.spoken(
            t_ms,
            &one(reply(&format!("{id}.{}", code(l)), text)),
            SpeechPriority::Comfort,
            "cmd_report",
        )
    }

    /// « Répète » with nothing said yet.
    pub fn nothing_to_repeat(&self, lang: CommandLang, t_ms: i64) -> SpokenView {
        let l: Lang = lang.into();
        let text = match l {
            Lang::Fr => "Rien à répéter.",
            Lang::En => "Nothing to repeat.",
        };
        self.spoken(
            t_ms,
            &one(reply(&format!("nothing-to-repeat.{}", code(l)), text)),
            SpeechPriority::Comfort,
            "cmd_repeat",
        )
    }

    /// A command about the outing with no outing followed.
    pub fn no_outing(&self, lang: CommandLang, t_ms: i64) -> SpokenView {
        let l: Lang = lang.into();
        let text = match l {
            Lang::Fr => "Aucune sortie en cours.",
            Lang::En => "No outing in progress.",
        };
        self.spoken(
            t_ms,
            &one(reply(&format!("no-outing.{}", code(l)), text)),
            SpeechPriority::Comfort,
            "cmd_no_outing",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracking::{GpsModeView, TurnDirection};

    fn guidance(off: f64, phase: PhaseView) -> GuidanceView {
        GuidanceView {
            phase,
            route: "plan".into(),
            label: "Boucle".into(),
            progress_m: 2000.0,
            length_m: 8000.0,
            remaining_m: 6000.0,
            next: Some(crate::tracking::NextTurn {
                direction: TurnDirection::Left,
                distance_m: 300.0,
                name: None,
            }),
            remaining_s: 3600.0,
            remaining_text: "1 h 00".into(),
            arrival: "15 h 00".into(),
            arrival_ms: 0,
            arrival_range: None,
            limit: None,
            off_route_m: off,
            gps_mode: GpsModeView::Pocket,
            proposal: None,
            switch: None,
            profile_proposal: None,
            back: None,
        }
    }

    #[test]
    fn classifies_through_the_ffi() {
        let h = classify_command("Où suis-je ?".into(), CommandLang::En);
        assert_eq!(h.command, Some(VoiceCommand::WhereAmI));
        assert_eq!(h.lang, CommandLang::Fr);
        let h = classify_command("Play some music".into(), CommandLang::En);
        assert_eq!(h.command, None);
        assert_eq!(command_questions(CommandLang::Fr).len(), 9);
        assert_eq!(command_questions(CommandLang::En)[3], "Take me back.");
    }

    #[test]
    fn where_am_i_on_and_off_the_route() {
        let v = VoiceAnswers::open(None);
        let on = v.where_am_i_at(&guidance(5.0, PhaseView::Moving), 0);
        assert_eq!(
            on.text,
            "Toujours sur l'itinéraire. Il reste six kilomètres. Environ une heure."
        );
        assert!(!on.all_clips);
        let off = v.where_am_i_at(&guidance(320.0, PhaseView::Moving), 0);
        assert!(
            off.text
                .starts_with("Vous avez quitté l'itinéraire. À trois cents mètres."),
            "{}",
            off.text
        );
        assert_eq!(off.segments[0].pause_ms, 400);
        let far = v.where_am_i_at(&guidance(9000.0, PhaseView::Moving), 0);
        assert!(
            far.text.contains("Il est à plus de deux kilomètres."),
            "{}",
            far.text
        );
        let none = v.where_am_i_at(&guidance(f64::INFINITY, PhaseView::Waiting), 0);
        assert!(
            none.text.starts_with("Il reste six kilomètres."),
            "{}",
            none.text
        );
        let done = v.where_am_i_at(&guidance(0.0, PhaseView::Finished), 0);
        assert_eq!(done.text, "Sortie terminée.");
    }

    #[test]
    fn battery_rounds_down_and_says_autonomy_when_known() {
        let v = VoiceAnswers::open(None);
        assert_eq!(
            v.battery(47.9, None, 0).text,
            "Batterie à quarante pour cent."
        );
        let s = v.battery(47.9, Some(3.0 * 3600.0), 0);
        assert!(
            s.text
                .starts_with("Batterie à quarante pour cent. Autonomie restante."),
            "{}",
            s.text
        );
        assert_eq!(
            v.battery(-3.0, Some(f64::NAN), 0).text,
            "Batterie à zéro pour cent."
        );
    }

    #[test]
    fn not_understood_lists_every_question() {
        let v = VoiceAnswers::open(None);
        let s = v.not_understood(CommandLang::Fr, 0);
        assert!(
            s.text
                .starts_with("Je n'ai pas compris. Vous pouvez demander :")
        );
        for q in command_questions(CommandLang::Fr) {
            assert!(s.text.contains(&q), "{q}");
        }
        let e = v.not_understood(CommandLang::En, 0);
        assert!(e.text.contains("Where am I?"));
        // Clip ids are valid file names.
        for seg in s.segments.iter().chain(&e.segments) {
            assert!(
                seg.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'-'),
                "{}",
                seg.id
            );
        }
    }

    #[test]
    fn replies_of_reports_repeat_and_no_outing() {
        let v = VoiceAnswers::open(None);
        assert_eq!(
            v.report(
                ReportKind::Groomed,
                ReportOutcome::Recorded,
                CommandLang::Fr,
                0
            )
            .text,
            "Signalement enregistré : piste damée."
        );
        assert!(
            v.report(
                ReportKind::TracksSet,
                ReportOutcome::Unavailable,
                CommandLang::En,
                0
            )
            .text
            .starts_with("Reports are not available")
        );
        assert_eq!(
            v.nothing_to_repeat(CommandLang::Fr, 0).text,
            "Rien à répéter."
        );
        assert_eq!(
            v.no_outing(CommandLang::En, 0).text,
            "No outing in progress."
        );
    }

    #[test]
    fn floor_quarter_never_later() {
        let t = |h, m, s| {
            chrono::NaiveDate::from_ymd_opt(2026, 1, 10)
                .unwrap()
                .and_hms_opt(h, m, s)
                .unwrap()
        };
        assert_eq!(
            floor_quarter(t(14, 44, 0)),
            Clock {
                hour: 14,
                minute: 30
            }
        );
        assert_eq!(
            floor_quarter(t(14, 44, 59)),
            Clock {
                hour: 14,
                minute: 30
            }
        );
        assert_eq!(
            floor_quarter(t(14, 45, 0)),
            Clock {
                hour: 14,
                minute: 45
            }
        );
    }
}
