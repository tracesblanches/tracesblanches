//! Every clip as a phrase with its parameters, said in any language (VO6).
//!
//! The phrase bank ([`crate::bank`]) and the plan build their announcements in French.
//! A clip id names the phrase and its values (`turn.left.200`, `clock.1630`,
//! `fix.arrive`); [`Phrase::parse`] reads them back, and [`Phrase::clip`] says the same
//! phrase in another language with the same id. Names (pistes, places) keep their spoken
//! form: a French proper name is read as it is by the English voice ("Greffier, blue
//! trail.").

use crate::bank::Announcement;
use crate::clips::{self, Clip, DayMonth, Direction, Fixed, capitalise};
use crate::en;
use crate::lang::Lang;
use crate::names::Colour;
use crate::quantity::Clock;

/// A clip of the vocabulary with its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phrase {
    /// A fixed phrase.
    Fixed(Fixed),
    /// "In two hundred metres, turn left." / "Turn left."
    Turn(Direction, Option<u32>),
    /// "At the fork, keep to the blue trail."
    Fork(Colour),
    /// "In two hundred metres."
    Ahead(u32),
    /// "One hundred metres away."
    At(u32),
    /// "For two kilometres."
    Over(u32),
    /// "Four kilometres to go." / "Less than one hundred metres to go."
    Remain(Option<u32>),
    /// "About an hour." / "Less than a minute."
    About(Option<u32>),
    /// A clock time.
    Clock(Clock),
    /// Turnaround countdown, minutes.
    TurnaroundIn(u32),
    /// Slope ahead, degrees.
    Slope(u32),
    /// Temperature, °C.
    Temperature(i32),
    /// Wind, km/h.
    Wind(u32),
    /// BERA level.
    Bera(u8),
    /// Open pistes.
    PistesOpen(u32),
    /// Battery, percent.
    Battery(u32),
    /// End of a protection period.
    Until(DayMonth),
    /// Loop length, km.
    LoopLength(u32),
    /// Loop duration, minutes.
    LoopAbout(u32),
    /// Loop return time.
    LoopBack(Clock),
    /// A piste name: its clip id, spoken name and colour.
    Piste {
        /// Clip id (`piste.<slug>[.<couleur>]`).
        id: String,
        /// Name as spoken.
        spoken: String,
        /// Colour.
        colour: Option<Colour>,
    },
    /// Another name (refuge, place, zone, site).
    Place {
        /// Clip id (`name.<slug>`).
        id: String,
        /// Name as spoken.
        spoken: String,
    },
}

fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn clock_key(s: &str) -> Option<Clock> {
    if s.len() != 4 {
        return None;
    }
    let hour: u8 = num(&s[..2])?;
    let minute: u8 = num(&s[2..])?;
    (hour < 24 && minute < 60).then_some(Clock { hour, minute })
}

fn colour_fr(w: &str) -> Option<Colour> {
    Colour::ALL.into_iter().find(|c| c.word() == w)
}

/// Suffix of a piste clip text after the name, in `lang`.
fn piste_suffix(colour: Option<Colour>, lang: Lang) -> String {
    match (colour, lang) {
        (Some(c), Lang::Fr) => format!(", piste {}.", c.word()),
        (Some(c), Lang::En) => format!(", {} trail.", en::colour(c)),
        (None, _) => ".".to_owned(),
    }
}

impl Phrase {
    /// The phrase of a clip whose text is in `lang` (the text is needed for names only).
    /// `None` for an id outside the vocabulary.
    pub fn parse(clip: &Clip, lang: Lang) -> Option<Phrase> {
        let id = clip.id.as_str();
        let (head, rest) = id.split_once('.')?;
        Some(match head {
            "fix" => Phrase::Fixed(*Fixed::ALL.iter().find(|f| f.clip().id == id)?),
            "turn" => {
                let (dir, v) = rest.rsplit_once('.')?;
                let d = Direction::ALL
                    .into_iter()
                    .find(|d| clips::turn(*d, None).id == format!("turn.{dir}.now"))?;
                if v == "now" {
                    Phrase::Turn(d, None)
                } else {
                    Phrase::Turn(d, Some(num(v)?))
                }
            }
            "fork" => Phrase::Fork(colour_fr(rest)?),
            "ahead" => Phrase::Ahead(num(rest)?),
            "at" => Phrase::At(num(rest)?),
            "over" => Phrase::Over(num(rest)?),
            "remain" if rest == "lt100" => Phrase::Remain(None),
            "remain" => Phrase::Remain(Some(num(rest)?)),
            "about" if rest == "lt1" => Phrase::About(None),
            "about" => Phrase::About(Some(num(rest)?)),
            "clock" => Phrase::Clock(clock_key(rest)?),
            "turnaround" => Phrase::TurnaroundIn(num(rest.strip_prefix("in.")?)?),
            "slope" => Phrase::Slope(num(rest)?),
            "temp" => match rest.strip_prefix('m') {
                Some(n) => Phrase::Temperature(-num::<i32>(n)?),
                None => Phrase::Temperature(num(rest)?),
            },
            "wind" => Phrase::Wind(num(rest)?),
            "bera" => Phrase::Bera(num(rest)?),
            "open" => Phrase::PistesOpen(num(rest)?),
            "battery" => Phrase::Battery(num(rest)?),
            "until" if rest.len() == 4 => Phrase::Until(DayMonth {
                month: num(&rest[..2])?,
                day: num(&rest[2..])?,
            }),
            "loop" => {
                let (k, v) = rest.split_once('.')?;
                match k {
                    "km" => Phrase::LoopLength(num(v)?),
                    "about" => Phrase::LoopAbout(num(v)?),
                    "back" => Phrase::LoopBack(clock_key(v)?),
                    _ => return None,
                }
            }
            "piste" => {
                let colour = rest
                    .rsplit_once('.')
                    .and_then(|(_, c)| colour_fr(c))
                    .filter(|_| rest.contains('.'));
                let spoken = clip
                    .text
                    .strip_suffix(&piste_suffix(colour, lang))?
                    .to_owned();
                Phrase::Piste {
                    id: id.to_owned(),
                    spoken,
                    colour,
                }
            }
            "name" => Phrase::Place {
                id: id.to_owned(),
                spoken: clip.text.strip_suffix('.')?.to_owned(),
            },
            _ => return None,
        })
    }

    /// The clip of the phrase in `lang` (same id in every language).
    pub fn clip(&self, lang: Lang) -> Clip {
        match lang {
            Lang::Fr => self.french(),
            Lang::En => self.english(),
        }
    }

    fn french(&self) -> Clip {
        match self {
            Phrase::Fixed(f) => f.clip(),
            Phrase::Turn(d, m) => clips::turn(*d, *m),
            Phrase::Fork(c) => clips::fork(*c),
            Phrase::Ahead(m) => clips::ahead(*m),
            Phrase::At(m) => clips::at(*m),
            Phrase::Over(m) => clips::over(*m),
            Phrase::Remain(m) => clips::remain(*m),
            Phrase::About(m) => clips::about(*m),
            Phrase::Clock(c) => clips::clock(*c),
            Phrase::TurnaroundIn(m) => clips::turnaround_in(*m),
            Phrase::Slope(d) => clips::slope(*d),
            Phrase::Temperature(t) => clips::temperature(*t),
            Phrase::Wind(w) => clips::wind(*w),
            Phrase::Bera(l) => clips::bera(*l),
            Phrase::PistesOpen(n) => clips::pistes_open(*n),
            Phrase::Battery(p) => clips::battery(*p),
            Phrase::Until(d) => clips::until(*d),
            Phrase::LoopLength(k) => clips::loop_length(*k),
            Phrase::LoopAbout(m) => clips::loop_about(*m),
            Phrase::LoopBack(c) => clips::loop_back(*c),
            Phrase::Piste { id, spoken, colour } => Clip {
                id: id.clone(),
                text: format!("{spoken}{}", piste_suffix(*colour, Lang::Fr)),
            },
            Phrase::Place { id, spoken } => Clip {
                id: id.clone(),
                text: format!("{spoken}."),
            },
        }
    }

    fn english(&self) -> Clip {
        let id = self.french().id;
        let text = match self {
            Phrase::Fixed(f) => f.english().to_owned(),
            Phrase::Turn(d, Some(m)) => {
                format!("In {}, {}.", en::distance_words(*m), en::verb(*d))
            }
            Phrase::Turn(d, None) => format!("{}.", capitalise(en::verb(*d))),
            Phrase::Fork(c) => format!("At the fork, keep to the {} trail.", en::colour(*c)),
            Phrase::Ahead(m) => format!("In {}.", en::distance_words(*m)),
            Phrase::At(m) => format!("{} away.", capitalise(&en::distance_words(*m))),
            Phrase::Over(m) => format!("For {}.", en::distance_words(*m)),
            Phrase::Remain(Some(m)) => {
                format!("{} to go.", capitalise(&en::distance_words(*m)))
            }
            Phrase::Remain(None) => "Less than one hundred metres to go.".to_owned(),
            Phrase::About(Some(m)) => format!("About {}.", en::duration_words(*m)),
            Phrase::About(None) => "Less than a minute.".to_owned(),
            Phrase::Clock(c) => format!("{}.", capitalise(&en::clock_words(*c))),
            Phrase::TurnaroundIn(m) => format!(
                "Turn back in {}, to be home before dark.",
                en::duration_words(*m)
            ),
            Phrase::Slope(d) => format!("Caution, slope of {} degrees ahead.", en::cardinal(*d)),
            Phrase::Temperature(t) => {
                let unit = if *t == 1 { "degree" } else { "degrees" };
                format!("Temperature, {} {unit}.", en::signed(*t))
            }
            Phrase::Wind(w) => format!("Wind, {} kilometres per hour.", en::cardinal(*w)),
            Phrase::Bera(l) => format!(
                "Avalanche danger {} out of five, {}.",
                en::cardinal(u32::from(*l)),
                en::bera_name(*l)
            ),
            Phrase::PistesOpen(0) => "No trails open.".to_owned(),
            Phrase::PistesOpen(1) => "One trail open.".to_owned(),
            Phrase::PistesOpen(n) => format!("{} trails open.", capitalise(&en::cardinal(*n))),
            Phrase::Battery(p) => format!("Battery at {} percent.", en::cardinal(*p)),
            Phrase::Until(d) => en::until(*d),
            Phrase::LoopLength(1) => "A one kilometre loop,".to_owned(),
            Phrase::LoopLength(k) => {
                // « An eleven », « an eighteen », « an eighty » kilometre loop.
                let n = en::cardinal(*k);
                let a = if n.starts_with('e') { "An" } else { "A" };
                format!("{a} {n} kilometre loop,")
            }
            Phrase::LoopAbout(m) => format!("about {},", en::duration_words(*m)),
            Phrase::LoopBack(c) => format!("back around {}.", en::clock_spoken(*c)),
            Phrase::Piste { spoken, colour, .. } => {
                format!("{spoken}{}", piste_suffix(*colour, Lang::En))
            }
            Phrase::Place { spoken, .. } => format!("{spoken}."),
        };
        Clip { id, text }
    }
}

/// The clip `clip`, whose text is in `from`, said in `to`; unchanged when its id is not
/// in the vocabulary.
pub fn localise(clip: &Clip, from: Lang, to: Lang) -> Clip {
    if from == to {
        return clip.clone();
    }
    Phrase::parse(clip, from).map_or_else(|| clip.clone(), |p| p.clip(to))
}

impl Announcement {
    /// The same announcement in `lang` (the bank speaks French): same clips, same pauses,
    /// the texts of `lang`.
    pub fn in_lang(&self, lang: Lang) -> Announcement {
        let mut a = self.clone();
        for s in &mut a.segments {
            s.clip = localise(&s.clip, Lang::Fr, lang);
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::PisteName;

    #[test]
    fn english_value_clips() {
        let en = |c: Clip| localise(&c, Lang::Fr, Lang::En).text;
        assert_eq!(
            en(clips::turn(Direction::Left, Some(200))),
            "In two hundred metres, turn left."
        );
        assert_eq!(en(clips::turn(Direction::UTurn, None)), "Turn back.");
        assert_eq!(en(clips::remain(Some(4000))), "Four kilometres to go.");
        assert_eq!(en(clips::about(Some(60))), "About one hour.");
        assert_eq!(
            en(clips::clock(Clock {
                hour: 16,
                minute: 30
            })),
            "Four thirty in the afternoon."
        );
        assert_eq!(
            en(clips::temperature(-8)),
            "Temperature, minus eight degrees."
        );
        assert_eq!(en(clips::temperature(1)), "Temperature, one degree.");
        assert_eq!(
            en(clips::bera(3)),
            "Avalanche danger three out of five, considerable."
        );
        assert_eq!(
            en(clips::until(DayMonth { day: 1, month: 7 })),
            "Applies until the first of July."
        );
        assert_eq!(
            en(clips::loop_back(Clock {
                hour: 12,
                minute: 15
            })),
            "back around a quarter past twelve."
        );
        let p = PisteName::new("Greffier", Some(Colour::Blue));
        let c = Clip {
            id: p.clip_id(),
            text: p.clip_text(),
        };
        let e = localise(&c, Lang::Fr, Lang::En);
        assert_eq!(
            (e.id.as_str(), e.text.as_str()),
            (c.id.as_str(), "Greffier, blue trail.")
        );
        assert_eq!(localise(&e, Lang::En, Lang::Fr), c);
        assert_eq!(
            localise(&Fixed::Arrive.clip(), Lang::Fr, Lang::En).text,
            "You have arrived at your destination."
        );
    }

    /// Every clip of a pack parses back to its phrase: the French text is unchanged, the
    /// English one has the same id, its own words and the same name.
    #[test]
    fn every_pack_clip_in_both_languages() {
        use crate::manifest::{PackNames, manifest, manifest_in};
        let names = PackNames {
            site: "Chapelle-des-Bois".into(),
            pistes: vec![
                PisteName::new("Greffier", Some(Colour::Black)),
                PisteName::new("Les Mortes (piste 2)", None),
                PisteName::new("Combe des Cives", Some(Colour::Green)),
            ],
            places: vec![
                "Refuge des Fonts de Cervières".into(),
                "APPB du Bois".into(),
            ],
            dates: vec![
                DayMonth { day: 30, month: 6 },
                DayMonth { day: 1, month: 3 },
            ],
            status_pistes: 25,
        };
        let fr = manifest(&names);
        let en = manifest_in(&names, Lang::En);
        assert_eq!(fr.clips.len(), en.clips.len());
        for (id, (kind, text)) in &fr.clips {
            let c = Clip {
                id: id.clone(),
                text: text.clone(),
            };
            let p = Phrase::parse(&c, Lang::Fr).unwrap_or_else(|| panic!("{id} does not parse"));
            assert_eq!(p.clip(Lang::Fr), c, "French round trip");
            let (ekind, etext) = &en.clips[id];
            assert_eq!(ekind, kind);
            // The same word in both languages: klister, the names of places and of pistes
            // without a colour.
            let same = id == "fix.kick-klister"
                || id.starts_with("name.")
                || matches!(p, Phrase::Piste { colour: None, .. });
            assert!(same || etext != text, "{id} not translated");
            let e = Clip {
                id: id.clone(),
                text: etext.clone(),
            };
            assert_eq!(
                localise(&e, Lang::En, Lang::Fr),
                c,
                "English back to French"
            );
            if *kind != crate::manifest::ClipKind::Name {
                assert!(etext.is_ascii(), "{id}: {etext}");
            }
            assert!(
                etext.ends_with('.') || etext.ends_with(',') || etext.ends_with('?'),
                "{id}: {etext}"
            );
        }
    }

    #[test]
    fn unknown_ids_are_kept() {
        let c = Clip {
            id: "reply.x".into(),
            text: "Texte.".into(),
        };
        assert_eq!(localise(&c, Lang::Fr, Lang::En), c);
    }
}
