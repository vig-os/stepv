//! The viewer's side of measurements (#33): a worker thread that owns the
//! [`crate::measure::Server`], so a query (and the kernel's first load of
//! the file) never blocks a frame. Queries go in tagged; answers come back
//! with their tag, polled once a frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use super::gpu::Pick;
use crate::measure::{Answer, Cap, Entity, Error, Query, Server};
use crate::occt::Limits;

/// The entity a pick names, `None` for a whole part (the tree's pick).
#[must_use]
pub fn entity_of(p: Pick) -> Option<Entity> {
    if p.is_whole_part() {
        None
    } else if let Some(edge) = p.edge_id() {
        Some(Entity::Edge { part: p.part, edge })
    } else {
        Some(Entity::Face {
            part: p.part,
            face: p.face,
        })
    }
}

/// A measurement server on its own thread.
pub struct Measurer {
    tx: Sender<(u64, Query)>,
    rx: Receiver<(u64, Result<Answer, Error>)>,
}

/// The queries of `batch` still worth asking: the newest measurement pair
/// (a later pick made the rest stale, and a slow one must not hold up the
/// next, #33 review), and the newest section (#43: the slider moved on).
fn fresh(batch: Vec<(u64, Query)>) -> Vec<(u64, Query)> {
    let section = |q: &Query| matches!(q, Query::Section(_));
    let newest = |want: bool| {
        batch
            .iter()
            .filter(|(_, q)| section(q) == want)
            .map(|(id, _)| *id)
            .max()
    };
    let (pair, cut) = (newest(false).unwrap_or(0), newest(true));
    batch
        .into_iter()
        .filter(|(id, q)| {
            if section(q) {
                Some(*id) == cut
            } else {
                id + 1 >= pair
            }
        })
        .collect()
}

impl Measurer {
    /// Starts the worker; the kernel starts on the first query.
    #[must_use]
    pub fn spawn(kernel: &Path, input: &Path, limits: Limits) -> Self {
        let (tx, jobs) = mpsc::channel::<(u64, Query)>();
        let (done, rx) = mpsc::channel();
        let (kernel, input): (PathBuf, PathBuf) = (kernel.to_owned(), input.to_owned());
        std::thread::spawn(move || {
            let mut server = Server::new(&kernel, &input, limits);
            while let Ok(first) = jobs.recv() {
                let mut batch = vec![first];
                batch.extend(jobs.try_iter());
                for (id, q) in fresh(batch) {
                    if done.send((id, server.ask(&q))).is_err() {
                        return;
                    }
                }
            }
        });
        Self { tx, rx }
    }

    pub fn send(&self, id: u64, q: Query) {
        let _ = self.tx.send((id, q));
    }

    /// An answer, if one is in.
    #[must_use]
    pub fn try_recv(&self) -> Option<(u64, Result<Answer, Error>)> {
        self.rx.try_recv().ok()
    }
}

/// Two picks and what the kernel said about them.
#[derive(Debug, Default)]
pub struct Measurement {
    pub a: Option<Pick>,
    pub b: Option<Pick>,
    pub distance: Option<Result<Answer, String>>,
    pub angle: Option<Result<Answer, String>>,
    /// The tags of the queries in flight: (distance, angle).
    pub waiting: Option<(u64, u64)>,
}

impl Measurement {
    /// Takes a pick: the first entity, then the second (which asks), then a
    /// new first. Returns the queries to send, with their tags from `next`.
    pub fn pick(&mut self, p: Pick, next: &mut u64) -> Vec<(u64, Query)> {
        let Some(e) = entity_of(p) else {
            return Vec::new();
        };
        match (self.a.and_then(entity_of), self.b) {
            (Some(a), None) => {
                self.b = Some(p);
                let (d, g) = (*next, *next + 1);
                *next += 2;
                self.waiting = Some((d, g));
                vec![(d, Query::Distance(a, e)), (g, Query::Angle(a, e))]
            }
            _ => {
                *self = Self {
                    a: Some(p),
                    ..Self::default()
                };
                Vec::new()
            }
        }
    }

    /// Files an answer; stale ones (an earlier pair's) are dropped. True
    /// when it completed the pair.
    pub fn answer(&mut self, id: u64, r: Result<Answer, Error>) -> bool {
        let Some((d, g)) = self.waiting else {
            return false;
        };
        let r = r.map_err(|e| e.to_string());
        if id == d {
            self.distance = Some(r);
        } else if id == g {
            self.angle = Some(r);
        } else {
            return false;
        }
        if self.distance.is_some() && self.angle.is_some() {
            self.waiting = None;
            return true;
        }
        false
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// How long the section plane holds still before the kernel is asked for
/// its exact caps (#43): a moving slider would queue a query per frame.
pub const SECTION_SETTLE: Duration = Duration::from_millis(150);

/// An answer, sorted by [`ExactCut::take`].
#[derive(Debug)]
pub enum Taken {
    /// The section's: the plane it was asked about, and its caps.
    Section([f32; 4], Result<Vec<Cap>, Error>),
    /// Anyone else's (a measurement's).
    Other(Result<Answer, Error>),
}

/// What the exact caps want this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Due {
    Nothing,
    /// Ask again after this long: the plane has not held still yet.
    Wait(Duration),
    /// Ask the kernel about this plane now.
    Ask([f32; 4]),
}

/// The exact section caps (#43): asked of the kernel once the plane holds
/// still; the stencil cap meanwhile, and wherever the kernel says no.
#[derive(Debug, Default)]
pub struct ExactCut {
    /// The capped plane last drawn (`None`: no cut, or uncapped), and since
    /// when.
    seen: Option<(Option<[f32; 4]>, Instant)>,
    /// The query in flight, and the plane it asks about.
    asked: Option<(u64, [f32; 4])>,
    /// The kernel said no for this plane. Cleared when the plane moves: the
    /// next stop asks again.
    refused: Option<([f32; 4], String)>,
    /// Too slow or too big for this model (a timeout, the memory cap, more
    /// than the GPU holds): no more queries this session, so every stop of
    /// the slider does not cost a kernel restart.
    off: Option<String>,
    /// How many times caps have landed: a render key's generation.
    pub landed: u64,
}

impl ExactCut {
    /// What to do for `plane` (`None`: no capped cut) this frame; `have`:
    /// the scene has its exact caps.
    pub fn due(&mut self, plane: Option<[f32; 4]>, have: bool, now: Instant) -> Due {
        if self.seen.is_none_or(|(p, _)| p != plane) {
            self.seen = Some((plane, now));
            self.refused = None;
        }
        let Some(p) = plane else {
            return Due::Nothing;
        };
        if have
            || self.off.is_some()
            || self.asked.is_some_and(|(_, a)| a == p)
            || self.refused.as_ref().is_some_and(|(r, _)| *r == p)
        {
            return Due::Nothing;
        }
        let held = self
            .seen
            .map_or(Duration::ZERO, |(_, t)| now.saturating_duration_since(t));
        if held < SECTION_SETTLE {
            Due::Wait(SECTION_SETTLE - held)
        } else {
            Due::Ask(p)
        }
    }

    pub fn sent(&mut self, id: u64, plane: [f32; 4]) {
        self.asked = Some((id, plane));
    }

    /// Whether the query in flight is still waited for.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.asked.is_some()
    }

    /// Sorts answer `id`: the section's, with its plane and caps, or any
    /// other, untouched.
    pub fn take(&mut self, id: u64, r: Result<Answer, Error>) -> Taken {
        match self.asked {
            Some((asked, plane)) if asked == id => {
                self.asked = None;
                Taken::Section(plane, r.map(|a| a.caps.unwrap_or_default()))
            }
            _ => Taken::Other(r),
        }
    }

    /// The caps for `plane` were set.
    pub fn landed(&mut self) {
        self.landed += 1;
    }

    /// No caps for `plane`, because of `e`. `sticky`: no more for this
    /// model (a timeout, the memory cap, too big).
    pub fn refuse(&mut self, plane: [f32; 4], e: String, sticky: bool) {
        if sticky {
            self.off = Some(e);
        } else {
            self.refused = Some((plane, e));
        }
    }

    /// Whether `plane`'s cap is the last it will be (the screenshot hook
    /// waits for it): exact, refused, or no cap at all.
    #[must_use]
    pub fn settled(&self, plane: Option<[f32; 4]>, have: bool) -> bool {
        plane.is_none_or(|p| {
            have || self.off.is_some() || self.refused.as_ref().is_some_and(|(r, _)| *r == p)
        })
    }

    /// The Section panel's line about `plane`'s cap, and why when it is not
    /// exact.
    #[must_use]
    pub fn status(&self, plane: [f32; 4], have: bool) -> (&'static str, Option<&str>) {
        if have {
            ("Exact section", None)
        } else if let Some(e) = &self.off {
            ("Approximate section", Some(e.as_str()))
        } else if let Some((_, e)) = self.refused.as_ref().filter(|(r, _)| *r == plane) {
            ("Approximate section", Some(e.as_str()))
        } else {
            ("Approximate section, exact on its way", None)
        }
    }
}

/// Whether a section that failed with `e` should stop the asking: a kernel
/// killed for its time or memory will be again.
#[must_use]
pub fn sticky(e: &Error) -> bool {
    matches!(
        e,
        Error::Timeout | Error::MemoryCap | Error::TooLong | Error::Load(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(distance: f64) -> Answer {
        Answer {
            distance: Some(distance),
            ..Answer::default()
        }
    }

    #[test]
    fn two_picks_ask_distance_and_angle() {
        let mut m = Measurement::default();
        let mut next = 1;
        assert!(m.pick(Pick { part: 0, face: 3 }, &mut next).is_empty());
        let q = m.pick(Pick::edge(1, 7), &mut next);
        let (a, b) = (
            Entity::Face { part: 0, face: 3 },
            Entity::Edge { part: 1, edge: 7 },
        );
        assert_eq!(q, vec![(1, Query::Distance(a, b)), (2, Query::Angle(a, b))]);
        assert_eq!(m.waiting, Some((1, 2)));
        m.answer(1, Ok(answer(4.0)));
        assert!(m.waiting.is_some(), "still waiting for the angle");
        m.answer(2, Err(Error::Refused("no axis".into())));
        assert!(m.waiting.is_none());
        assert_eq!(
            m.distance.as_ref().unwrap().as_ref().unwrap().distance,
            Some(4.0)
        );
        assert_eq!(m.angle, Some(Err("no axis".into())));
        // A third pick starts over.
        assert!(m.pick(Pick { part: 2, face: 0 }, &mut next).is_empty());
        assert!(m.b.is_none() && m.distance.is_none());
    }

    #[test]
    fn a_section_is_asked_once_the_plane_holds_still() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let (p, q) = ([0.0, 1.0, 0.0, 15.0], [0.0, 1.0, 0.0, 14.0]);
        let mut x = ExactCut::default();
        assert_eq!(x.due(None, false, t0), Due::Nothing);
        assert_eq!(x.due(Some(p), false, ms(0)), Due::Wait(SECTION_SETTLE));
        // The slider moves on: the wait starts over.
        assert_eq!(x.due(Some(q), false, ms(100)), Due::Wait(SECTION_SETTLE));
        assert_eq!(x.due(Some(q), false, ms(260)), Due::Ask(q));
        x.sent(7, q);
        assert_eq!(
            x.due(Some(q), false, ms(300)),
            Due::Nothing,
            "asked already"
        );
        assert!(!x.settled(Some(q), false), "the screenshot waits");
        // Another query's answer is not the section's.
        assert!(matches!(
            x.take(6, Ok(Answer::default())),
            Taken::Other(Ok(_))
        ));
        let Taken::Section(plane, caps) = x.take(7, Ok(Answer::default())) else {
            panic!("the section's answer");
        };
        assert_eq!((plane, caps), (q, Ok(Vec::new())));
        assert!(!x.waiting());
        assert!(x.settled(Some(q), true));
        assert_eq!(x.status(q, true).0, "Exact section");
    }

    #[test]
    fn a_refusal_holds_until_the_plane_moves_and_a_timeout_for_good() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let (p, q) = ([0.0, 1.0, 0.0, 15.0], [0.0, 1.0, 0.0, 14.0]);
        let mut x = ExactCut::default();
        x.due(Some(p), false, ms(0));
        assert_eq!(x.due(Some(p), false, ms(200)), Due::Ask(p));
        x.sent(1, p);
        let Taken::Section(plane, Err(e)) = x.take(1, Err(Error::Refused("no".into()))) else {
            panic!("the section's refusal");
        };
        x.refuse(plane, e.to_string(), false);
        assert_eq!(x.due(Some(p), false, ms(400)), Due::Nothing);
        assert!(x.settled(Some(p), false));
        assert_eq!(x.status(p, false), ("Approximate section", Some("no")));
        // Moved away and back: asked again.
        x.due(Some(q), false, ms(500));
        x.due(Some(p), false, ms(600));
        assert_eq!(x.due(Some(p), false, ms(800)), Due::Ask(p));
        // A timeout stops the asking for this model.
        assert!(sticky(&Error::Timeout) && !sticky(&Error::Refused(String::new())));
        x.refuse(p, Error::Timeout.to_string(), true);
        x.due(Some(q), false, ms(900));
        assert_eq!(x.due(Some(q), false, ms(1200)), Due::Nothing);
        assert!(x.settled(Some(q), false));
    }

    #[test]
    fn a_late_refusal_for_an_old_plane_does_not_block_the_new_one() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let (p, q) = ([0.0, 1.0, 0.0, 15.0], [0.0, 1.0, 0.0, 14.0]);
        let mut x = ExactCut::default();
        x.due(Some(p), false, ms(0));
        assert_eq!(x.due(Some(p), false, ms(200)), Due::Ask(p));
        x.sent(1, p);
        // The slider moves on while p's query is out; then p is refused.
        x.due(Some(q), false, ms(300));
        let Taken::Section(plane, Err(e)) = x.take(1, Err(Error::Crashed("x".into()))) else {
            panic!("the section's answer");
        };
        x.refuse(plane, e.to_string(), sticky(&e));
        assert_eq!(
            x.due(Some(q), false, ms(500)),
            Due::Ask(q),
            "q is still asked"
        );
        assert!(!x.settled(Some(q), false));
    }

    #[test]
    fn only_the_newest_pair_and_section_are_asked() {
        let (a, b) = (
            Entity::Face { part: 0, face: 0 },
            Entity::Face { part: 0, face: 1 },
        );
        let cut = |w| Query::Section([0.0, 1.0, 0.0, w]);
        let batch = vec![
            (1, Query::Distance(a, b)),
            (2, Query::Angle(a, b)),
            (3, cut(1.0)),
            (4, cut(2.0)),
            (5, Query::Distance(b, a)),
            (6, Query::Angle(b, a)),
        ];
        let ids: Vec<u64> = fresh(batch).iter().map(|(id, _)| *id).collect();
        // A newer pair does not make the section stale, nor the other way.
        assert_eq!(ids, [4, 5, 6]);
        let ids: Vec<u64> = fresh(vec![
            (7, Query::Distance(a, b)),
            (8, Query::Angle(a, b)),
            (9, cut(3.0)),
        ])
        .iter()
        .map(|(id, _)| *id)
        .collect();
        assert_eq!(ids, [7, 8, 9]);
    }

    #[test]
    fn stale_answers_and_whole_parts_are_ignored() {
        let mut m = Measurement::default();
        let mut next = 10;
        assert!(m.pick(Pick::part(4), &mut next).is_empty());
        assert!(m.a.is_none(), "a whole part is not an entity");
        m.pick(Pick { part: 0, face: 0 }, &mut next);
        m.pick(Pick { part: 0, face: 1 }, &mut next);
        m.answer(3, Ok(answer(1.0)));
        assert!(m.distance.is_none(), "an answer to an older pair");
        assert_eq!(
            entity_of(Pick::edge(2, 5)),
            Some(Entity::Edge { part: 2, edge: 5 })
        );
    }
}
