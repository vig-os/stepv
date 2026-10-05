//! The viewer's side of measurements (#33): a worker thread that owns the
//! [`crate::measure::Server`], so a query (and the kernel's first load of
//! the file) never blocks a frame. Queries go in tagged; answers come back
//! with their tag, polled once a frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use super::gpu::Pick;
use crate::measure::{Answer, Entity, Error, Query, Server};
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
                // Only the newest pair: a later pick made the rest stale, and
                // a slow one must not hold up the next (#33 review).
                let mut batch = vec![first];
                batch.extend(jobs.try_iter());
                let newest = batch.iter().map(|(id, _)| *id).max().unwrap_or(0);
                for (id, q) in batch {
                    if id + 1 < newest {
                        continue;
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(distance: f64) -> Answer {
        Answer {
            distance: Some(distance),
            points: None,
            axis_distance: None,
            angle_deg: None,
            point: None,
            normal: None,
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
