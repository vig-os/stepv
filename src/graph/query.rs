//! jq filters over the graph (ADR-0001 §2.4), by `jaq`.
//!
//! The view is a set of native filters over plain `jaq_json` values, the
//! ADR's fallback to a lazy `ValT` (§2.4 records why and the measured heap).
//! Each materialises only what it is called for:
//!
//! - `summary`, `census`, `products`, `tree`: the top level, as
//!   [`View`] writes it (`tree` is the whole outline, under its caps);
//! - `entity($id)`: one entity's `{"id", "type", "args"}`, its arguments
//!   parsed then;
//! - `refs($id)`, `referrers($id)`: step ids.
//!
//! The input `.` is null. File text arrives as `{"untrusted": "…"}`. A filter
//! sees nothing of the host and can't end the process: `env`, `halt` and the
//! logging natives raise an error instead, and there is no `input`.

use std::fmt;

use jaq_core::box_iter::box_once;
use jaq_core::data::{DataT, HasLut};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::native::{Filter, Fun, run, v};
use jaq_core::{Compiler, Ctx, Cv, Error, Exn, Lut, RunPtr, ValXs, Vars};
use jaq_json::{Num, Val};
use serde::Deserialize as _;
use serde_json::Value as Json;

use super::json::{self, TreeOptions, View};

/// Why a filter has no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// The filter doesn't parse or compile; the message points into it.
    Filter(String),
    /// The filter failed while it ran.
    Runtime(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Filter(e) => write!(f, "invalid filter: {e}"),
            Self::Runtime(e) => write!(f, "filter failed: {e}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// The outputs of [`query`], at most its limit.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// The filter's outputs, in order.
    pub results: Vec<Json>,
    /// Whether the filter had more outputs than the limit.
    pub more: bool,
}

/// The data the natives see: the compiled filter's table and the view.
struct Env<'a> {
    lut: &'a Lut<Graphs>,
    view: &'a View<'a>,
}

/// The filters' data kind: [`jaq_json::Val`] over a borrowed [`Env`].
struct Graphs;

impl DataT for Graphs {
    type V<'a> = Val;
    type Data<'a> = &'a Env<'a>;
}

impl<'a> HasLut<'a, Graphs> for &'a Env<'a> {
    fn lut(&self) -> &'a Lut<Graphs> {
        self.lut
    }
}

/// Runs `filter` over `view`, keeping at most `limit` outputs.
///
/// # Errors
/// [`QueryError::Filter`] when the filter doesn't parse or compile,
/// [`QueryError::Runtime`] when it raises an error.
pub fn query(view: &View<'_>, filter: &str, limit: usize) -> Result<Answer, QueryError> {
    let refused = ["env", "halt", "stderr_empty", "debug_empty"];
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let funs = jaq_core::funs()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs())
        .filter(|(name, _, _)| !refused.contains(name))
        .chain(natives())
        .chain(refused.iter().map(|&name| refuse(name)));
    let arena = Arena::default();
    let modules = Loader::new(defs)
        .load(
            &arena,
            File {
                code: filter,
                path: (),
            },
        )
        .map_err(|errs| QueryError::Filter(load_errors(filter, errs)))?;
    let compiled = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| {
            let undefined = errs
                .into_iter()
                .flat_map(|(_, e)| e)
                .map(|(name, u)| format!("undefined {} {name}", u.as_str()));
            QueryError::Filter(undefined.collect::<Vec<_>>().join("; "))
        })?;
    let env = Env {
        lut: &compiled.lut,
        view,
    };
    let ctx = Ctx::<Graphs>::new(&env, Vars::new([]));
    let mut out = compiled.id.run((ctx, Val::Null));
    let mut results = Vec::new();
    for x in out.by_ref().take(limit) {
        let v = x.map_err(|e| match e.get_err() {
            Ok(e) => QueryError::Runtime(e.to_string()),
            Err(_) => QueryError::Runtime("halted".into()),
        })?;
        results.push(to_json(&v));
    }
    let more = out.next().is_some();
    Ok(Answer { results, more })
}

/// The parse errors, each with its byte offset in the filter.
fn load_errors(code: &str, errs: jaq_core::load::Errors<&str, ()>) -> String {
    use jaq_core::load::Error as E;
    let at = |s: &str| jaq_core::load::span(code, s).start;
    let mut msgs = Vec::new();
    for (_, e) in errs {
        match e {
            E::Io(e) => msgs.extend(e.into_iter().map(|(_, e)| e)),
            E::Lex(e) => msgs.extend(
                e.into_iter()
                    .map(|(x, s)| format!("byte {}: expected {}", at(s), x.as_str())),
            ),
            E::Parse(e) => msgs.extend(
                e.into_iter()
                    .map(|(x, s)| format!("byte {}: expected {}", at(s), x.as_str())),
            ),
        }
    }
    msgs.join("; ")
}

/// The view's filters.
fn natives() -> impl Iterator<Item = Fun<Graphs>> {
    let fs: [Filter<RunPtr<Graphs>>; 7] = [
        ("summary", v(0), |cv| top(cv, |w| Ok(w.summary()))),
        ("census", v(0), |cv| {
            top(cv, |w| Ok(json::census(&w.graph().census())))
        }),
        ("products", v(0), |cv| {
            top(cv, |w| w.products().map_err(|e| e.to_string()))
        }),
        ("tree", v(0), |cv| {
            top(cv, |w| {
                let all = TreeOptions {
                    limit: u64::MAX,
                    ..TreeOptions::default()
                };
                let page = w.tree(all).map_err(|e| e.to_string())?;
                Ok(page["rows"].clone())
            })
        }),
        ("entity", v(1), |cv| by_id(cv, |w, id| w.entity(id))),
        ("refs", v(1), |cv| by_id(cv, |w, id| w.refs(id))),
        ("referrers", v(1), |cv| by_id(cv, |w, id| w.referrers(id))),
    ];
    fs.into_iter().map(run::<Graphs>)
}

/// A top-level filter: `f`'s value, whatever the input.
fn top<'a>(cv: Cv<'a, Graphs>, f: fn(&View<'_>) -> Result<Json, String>) -> ValXs<'a, Val> {
    box_once(f(cv.0.data().view).map(|j| to_val(&j)).map_err(err))
}

/// An entity-level filter: `f` of the step id in its argument.
fn by_id<'a>(mut cv: Cv<'a, Graphs>, f: fn(&View<'_>, u32) -> Json) -> ValXs<'a, Val> {
    let view = cv.0.data().view;
    box_once(
        step_id(&cv.0.pop_var())
            .map(|id| to_val(&f(view, id)))
            .map_err(err),
    )
}

/// A filter that only raises an error: what the host-facing natives become.
fn refuse(name: &'static str) -> Fun<Graphs> {
    let arity = if name == "halt" { 1 } else { 0 };
    run((name, v(arity), |_| {
        box_once(Err(err("not available in stepv graph query".into())))
    }))
}

fn err<'a>(msg: String) -> Exn<'a, Val> {
    Exn::from(Error::str(msg))
}

/// A step id: a non-negative integer that fits a `u32`.
fn step_id(v: &Val) -> Result<u32, String> {
    match v {
        Val::Num(Num::Int(i)) => u32::try_from(*i).ok(),
        _ => None,
    }
    .ok_or_else(|| format!("expected a step id, got {v}"))
}

fn to_val(j: &Json) -> Val {
    Val::deserialize(j).expect("serde_json values are JSON")
}

/// A jaq value as JSON, through jaq's own JSON text.
fn to_json(v: &Val) -> Json {
    serde_json::from_str(&v.to_string()).unwrap_or_else(|_| Json::String(v.to_string()))
}
