//! The model tree (#30): the assembly tree flattened once into rows, drawn
//! virtualised, with show/hide, isolate, search and selection.
//!
//! #27 measured the naive build, nested `CollapsingHeader`s, at 38 ms a
//! frame for 40k nodes. Here a frame costs only the rows on screen:
//! - the tree is one preorder vector, so a node's subtree is the range
//!   `i + 1..end`;
//! - the list of rows to show (expanded and matching the search) is
//!   rebuilt only when expansion or the search changes;
//! - an assembly's checkbox state comes from a shown-leaf count per row,
//!   recomputed only when visibility changes;
//! - `ScrollArea::show_rows` lays out only the visible slice.

use crate::topology::{Node, Topology};

/// One node of the flattened tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub depth: u16,
    /// A leaf's mesh part; `None` for an assembly.
    pub part: Option<u32>,
    pub parent: Option<u32>,
    /// One past this node's subtree: its descendants are `i + 1..end`.
    pub end: u32,
}

/// A checkbox's state for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    All,
    Some,
    None,
}

/// The flattened tree and the viewer's state over it.
#[derive(Debug, Clone)]
pub struct Tree {
    rows: Vec<Row>,
    /// Per row: expanded (assemblies; leaves ignore it).
    open: Vec<bool>,
    /// Per part: shown.
    shown: Vec<bool>,
    /// Per row: leaves below (itself, for a leaf) and how many are shown.
    leaves: Vec<u32>,
    shown_leaves: Vec<u32>,
    /// Lower-cased names, for the search.
    lower: Vec<String>,
    filter: String,
    /// The search field's text as typed (`filter` is its normalised form).
    #[cfg_attr(not(feature = "viewer"), allow(dead_code))]
    query: String,
    /// The rows to draw, in order: rebuilt by [`Self::refresh`].
    display: Vec<u32>,
    stale: bool,
    /// Bumped on every visibility change: the viewer re-uploads on a new one.
    pub generation: u64,
}

impl Tree {
    /// The tree `topo` describes over `parts` mesh parts.
    #[must_use]
    pub fn from_topology(topo: &Topology, parts: usize) -> Self {
        let mut rows = Vec::new();
        fn walk(n: &Node, depth: u16, parent: Option<u32>, rows: &mut Vec<Row>) {
            let i = rows.len() as u32;
            match n {
                Node::Part { name, part } => rows.push(Row {
                    name: name.clone(),
                    depth,
                    part: Some(*part as u32),
                    parent,
                    end: i + 1,
                }),
                Node::Assembly { name, children } => {
                    rows.push(Row {
                        name: name.clone(),
                        depth,
                        part: None,
                        parent,
                        end: 0,
                    });
                    for c in children {
                        walk(c, depth + 1, Some(i), rows);
                    }
                    rows[i as usize].end = rows.len() as u32;
                }
            }
        }
        for n in &topo.tree {
            walk(n, 0, None, &mut rows);
        }
        Self::new(rows, parts)
    }

    /// A flat list, one row per part: for a file without topology.
    #[must_use]
    pub fn flat(names: &[Option<String>]) -> Self {
        let rows = names
            .iter()
            .enumerate()
            .map(|(i, n)| Row {
                name: n.clone().unwrap_or_else(|| format!("part {i}")),
                depth: 0,
                part: Some(i as u32),
                parent: None,
                end: i as u32 + 1,
            })
            .collect();
        Self::new(rows, names.len())
    }

    #[must_use]
    pub fn new(rows: Vec<Row>, parts: usize) -> Self {
        let n = rows.len();
        // Top-level assemblies open; deeper ones closed, so a huge tree
        // starts short.
        let open = rows.iter().map(|r| r.depth == 0).collect();
        let lower = rows.iter().map(|r| r.name.to_lowercase()).collect();
        let mut t = Self {
            rows,
            open,
            shown: vec![true; parts],
            leaves: vec![0; n],
            shown_leaves: vec![0; n],
            lower,
            filter: String::new(),
            query: String::new(),
            display: Vec::new(),
            stale: true,
            generation: 0,
        };
        t.recount();
        t.refresh();
        t
    }

    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The rows to draw, in order.
    #[must_use]
    pub fn display(&self) -> &[u32] {
        &self.display
    }

    #[must_use]
    pub fn is_open(&self, row: u32) -> bool {
        self.open[row as usize]
    }

    /// Per part: shown.
    #[must_use]
    pub fn shown_parts(&self) -> &[bool] {
        &self.shown
    }

    #[must_use]
    pub fn shown(&self, row: u32) -> Shown {
        let (n, s) = (self.leaves[row as usize], self.shown_leaves[row as usize]);
        match s {
            0 => Shown::None,
            s if s == n => Shown::All,
            _ => Shown::Some,
        }
    }

    /// Recounts shown leaves bottom-up: preorder reversed visits children
    /// before their parent.
    fn recount(&mut self) {
        for i in (0..self.rows.len()).rev() {
            let (n, s) = match self.rows[i].part {
                Some(p) => (
                    1,
                    u32::from(self.shown.get(p as usize).copied().unwrap_or(false)),
                ),
                None => (0, 0),
            };
            self.leaves[i] += n;
            self.shown_leaves[i] += s;
            if let Some(parent) = self.rows[i].parent {
                self.leaves[parent as usize] += self.leaves[i];
                self.shown_leaves[parent as usize] += self.shown_leaves[i];
            }
        }
    }

    fn visibility_changed(&mut self) {
        self.leaves.iter_mut().for_each(|v| *v = 0);
        self.shown_leaves.iter_mut().for_each(|v| *v = 0);
        self.recount();
        self.generation += 1;
    }

    /// Shows or hides every part under `row`.
    pub fn set_shown(&mut self, row: u32, shown: bool) {
        let r = row as usize;
        for i in r..self.rows[r].end as usize {
            if let Some(p) = self.rows[i].part {
                self.shown[p as usize] = shown;
            }
        }
        self.visibility_changed();
    }

    /// Shows only the parts under `row`.
    pub fn isolate(&mut self, row: u32) {
        self.shown.iter_mut().for_each(|s| *s = false);
        self.set_shown(row, true);
    }

    pub fn show_all(&mut self) {
        self.shown.iter_mut().for_each(|s| *s = true);
        self.visibility_changed();
    }

    pub fn toggle_open(&mut self, row: u32) {
        let o = &mut self.open[row as usize];
        *o = !*o;
        self.stale = true;
    }

    /// The row of `part`'s leaf.
    #[must_use]
    pub fn row_of(&self, part: u32) -> Option<u32> {
        self.rows
            .iter()
            .position(|r| r.part == Some(part))
            .map(|i| i as u32)
    }

    /// Opens every ancestor of `row`, so it is displayed.
    pub fn reveal(&mut self, row: u32) {
        let mut p = self.rows[row as usize].parent;
        while let Some(i) = p {
            if !self.open[i as usize] {
                self.open[i as usize] = true;
                self.stale = true;
            }
            p = self.rows[i as usize].parent;
        }
    }

    /// Filters by a case-insensitive substring of the name; matches show
    /// with their ancestors, whatever is expanded. Empty shows everything.
    pub fn set_filter(&mut self, query: &str) {
        let q = query.trim().to_lowercase();
        if q != self.filter {
            self.filter = q;
            self.stale = true;
        }
    }

    #[must_use]
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Rebuilds the display list if expansion or the filter changed.
    pub fn refresh(&mut self) {
        if !self.stale {
            return;
        }
        self.stale = false;
        self.display.clear();
        if self.filter.is_empty() {
            let mut i = 0;
            while i < self.rows.len() {
                self.display.push(i as u32);
                let r = &self.rows[i];
                i = if r.part.is_none() && !self.open[i] {
                    r.end as usize
                } else {
                    i + 1
                };
            }
            return;
        }
        let mut keep = vec![false; self.rows.len()];
        for (i, name) in self.lower.iter().enumerate() {
            if name.contains(&self.filter) {
                keep[i] = true;
                let mut p = self.rows[i].parent;
                while let Some(a) = p {
                    if keep[a as usize] {
                        break;
                    }
                    keep[a as usize] = true;
                    p = self.rows[a as usize].parent;
                }
            }
        }
        self.display.extend(
            keep.iter()
                .enumerate()
                .filter(|(_, k)| **k)
                .map(|(i, _)| i as u32),
        );
    }
}

/// What the panel did this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Response {
    /// A part was clicked: select it.
    pub selected: Option<u32>,
    /// Rows laid out: only the visible slice, however big the tree.
    pub laid_out: usize,
}

#[cfg(feature = "viewer")]
impl Tree {
    /// Draws the tree into `ui`: a search field, Show all and Isolate, and
    /// the rows. `selected` is the selected part, highlighted;
    /// `scroll_to` brings its row into view once.
    pub fn panel(
        &mut self,
        ui: &mut eframe::egui::Ui,
        selected: Option<u32>,
        scroll_to: bool,
    ) -> Response {
        use super::{theme, widgets};
        use eframe::egui::{self, RichText};
        use egui_phosphor::regular as icon;

        let mut out = Response::default();
        let t = widgets::tokens(ui);
        let mut query = std::mem::take(&mut self.query);
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text(format!("{} Search parts", icon::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY),
        );
        self.set_filter(&query);
        self.query = query;
        ui.horizontal(|ui| {
            if ui.button("Show all").clicked() {
                self.show_all();
            }
            let sel_row = selected.and_then(|p| self.row_of(p));
            if ui
                .add_enabled(sel_row.is_some(), egui::Button::new("Isolate"))
                .on_hover_text("Show only the selected part")
                .clicked()
                && let Some(r) = sel_row
            {
                self.isolate(r);
            }
        });
        ui.add_space(theme::space(1));
        let sel_row = selected.and_then(|p| self.row_of(p));
        if scroll_to && let Some(r) = sel_row {
            self.reveal(r);
        }
        self.refresh();

        let row_h = ui.spacing().interact_size.y;
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if scroll_to
            && let Some(r) = sel_row
            && let Some(pos) = self.display.iter().position(|&d| d == r)
        {
            // Centre-ish: two rows above it stay visible.
            area = area.vertical_scroll_offset((pos.saturating_sub(2)) as f32 * row_h);
        }
        let display = std::mem::take(&mut self.display);
        area.show_rows(ui, row_h, display.len(), |ui, range| {
            for &i in &display[range] {
                out.laid_out += 1;
                let row = self.rows[i as usize].clone();
                ui.horizontal(|ui| {
                    ui.set_min_height(row_h);
                    ui.add_space(f32::from(row.depth) * theme::space(4));
                    if row.part.is_none() && self.filter.is_empty() {
                        let caret = if self.open[i as usize] {
                            icon::CARET_DOWN
                        } else {
                            icon::CARET_RIGHT
                        };
                        if ui
                            .add(egui::Button::new(caret).frame(false))
                            .on_hover_text("Expand or collapse")
                            .clicked()
                        {
                            self.toggle_open(i);
                        }
                    } else {
                        ui.add_space(theme::space(4));
                    }
                    let state = self.shown(i);
                    let mut on = state != Shown::None;
                    if ui
                        .add(
                            egui::Checkbox::without_text(&mut on)
                                .indeterminate(state == Shown::Some),
                        )
                        .on_hover_text("Show or hide")
                        .clicked()
                    {
                        self.set_shown(i, state != Shown::All);
                    }
                    let is_sel = row.part.is_some() && row.part == selected;
                    let label = RichText::new(&row.name).color(if row.part.is_some() {
                        t.foreground
                    } else {
                        t.muted_foreground
                    });
                    let resp = ui.selectable_label(is_sel, label);
                    if resp.clicked()
                        && let Some(p) = row.part
                    {
                        out.selected = Some(p);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Isolate").clicked() {
                            self.isolate(i);
                            ui.close();
                        }
                        if ui.button("Hide").clicked() {
                            self.set_shown(i, false);
                            ui.close();
                        }
                        if ui.button("Show all").clicked() {
                            self.show_all();
                            ui.close();
                        }
                    });
                });
            }
        });
        if self.display.is_empty() {
            self.display = display;
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// `groups` assemblies of `per` leaves each, under one root: 1 + groups
    /// + groups * per rows.
    pub(crate) fn synthetic(groups: u32, per: u32) -> Tree {
        let mut rows = vec![Row {
            name: "root".into(),
            depth: 0,
            part: None,
            parent: None,
            end: 0,
        }];
        let mut part = 0;
        for g in 0..groups {
            let gi = rows.len() as u32;
            rows.push(Row {
                name: format!("assembly {g}"),
                depth: 1,
                part: None,
                parent: Some(0),
                end: 0,
            });
            for _ in 0..per {
                rows.push(Row {
                    name: format!("part {part}"),
                    depth: 2,
                    part: Some(part),
                    parent: Some(gi),
                    end: rows.len() as u32 + 1,
                });
                part += 1;
            }
            rows[gi as usize].end = rows.len() as u32;
        }
        rows[0].end = rows.len() as u32;
        Tree::new(rows, part as usize)
    }

    fn names(t: &Tree) -> Vec<&str> {
        t.display()
            .iter()
            .map(|&i| t.rows()[i as usize].name.as_str())
            .collect()
    }

    #[test]
    fn closed_assemblies_hide_their_subtree() {
        let mut t = synthetic(2, 2);
        // The root is open, the groups closed.
        assert_eq!(names(&t), ["root", "assembly 0", "assembly 1"]);
        t.toggle_open(1);
        t.refresh();
        assert_eq!(
            names(&t),
            ["root", "assembly 0", "part 0", "part 1", "assembly 1"]
        );
        t.toggle_open(0);
        t.refresh();
        assert_eq!(names(&t), ["root"]);
    }

    #[test]
    fn hiding_an_assembly_hides_its_parts_and_counts_up() {
        let mut t = synthetic(2, 2);
        t.set_shown(1, false); // assembly 0
        assert_eq!(t.shown_parts(), &[false, false, true, true]);
        assert_eq!(t.shown(1), Shown::None);
        assert_eq!(t.shown(0), Shown::Some, "the root is mixed");
        t.set_shown(2, true); // part 0
        assert_eq!(t.shown(1), Shown::Some);
        t.show_all();
        assert_eq!(t.shown(0), Shown::All);
        assert!(t.generation >= 3, "each change bumps the generation");
    }

    #[test]
    fn isolate_shows_only_the_subtree() {
        let mut t = synthetic(3, 2);
        let a1 = t
            .rows()
            .iter()
            .position(|r| r.name == "assembly 1")
            .unwrap() as u32;
        t.isolate(a1);
        assert_eq!(t.shown_parts(), &[false, false, true, true, false, false]);
    }

    #[test]
    fn search_shows_matches_with_their_ancestors() {
        let mut t = synthetic(3, 4);
        t.set_filter("  PART 9 ");
        t.refresh();
        assert_eq!(names(&t), ["root", "assembly 2", "part 9"]);
        t.set_filter("");
        t.refresh();
        assert_eq!(
            names(&t),
            ["root", "assembly 0", "assembly 1", "assembly 2"]
        );
    }

    #[test]
    fn revealing_a_part_opens_its_ancestors() {
        let mut t = synthetic(3, 2);
        let r = t.row_of(5).unwrap();
        t.reveal(r);
        t.refresh();
        assert!(t.display().contains(&r));
    }

    #[test]
    fn a_flat_tree_names_unnamed_parts() {
        let t = Tree::flat(&[Some("plate".into()), None]);
        assert_eq!(names(&t), ["plate", "part 1"]);
    }

    #[test]
    fn the_display_list_is_cached() {
        let mut t = synthetic(400, 100);
        t.toggle_open(1);
        t.refresh();
        let before = t.display().as_ptr();
        // No change: no rebuild (the same allocation, same contents).
        t.refresh();
        assert_eq!(t.display().as_ptr(), before);
        assert_eq!(t.display().len(), 1 + 400 + 100);
    }

    /// Lays the panel out in a 260 x 800 headless frame; returns what it
    /// did and the frame's cost (layout and tessellation, as eframe's
    /// frame does).
    #[cfg(feature = "viewer")]
    fn frame(t: &mut Tree, ctx: &eframe::egui::Context) -> (Response, std::time::Duration) {
        use eframe::egui;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(260.0, 800.0),
            )),
            ..Default::default()
        };
        let mut r = Response::default();
        let t0 = std::time::Instant::now();
        let mut out = ctx.run_ui(input, |ui| {
            r = t.panel(ui, Some(7), false);
        });
        let _ = ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        out.textures_delta.clear();
        (r, t0.elapsed())
    }

    /// Opens every assembly.
    #[cfg(feature = "viewer")]
    fn open_all(t: &mut Tree) {
        for i in 0..t.rows().len() as u32 {
            if t.rows()[i as usize].part.is_none() && !t.is_open(i) {
                t.toggle_open(i);
            }
        }
    }

    #[cfg(feature = "viewer")]
    #[test]
    fn only_the_visible_rows_are_laid_out() {
        let mut t = synthetic(400, 100); // 40,401 rows
        open_all(&mut t);
        let ctx = eframe::egui::Context::default();
        super::super::theme::install(&ctx, super::super::ThemePref::Light);
        for _ in 0..3 {
            let (r, _) = frame(&mut t, &ctx);
            assert!(
                r.laid_out > 10 && r.laid_out < 60,
                "laid out {} rows",
                r.laid_out
            );
        }
        assert_eq!(
            t.display().len(),
            40_401,
            "the display list survives the frame"
        );
    }

    /// #30's acceptance: a 40k-node tree under 4 ms a frame. Release only
    /// (debug egui is many times slower):
    ///   cargo test --release --lib tree -- --ignored --nocapture
    #[cfg(feature = "viewer")]
    #[test]
    #[ignore = "a timing: run with --release"]
    fn forty_thousand_nodes_stay_under_four_ms() {
        let mut t = synthetic(400, 100);
        open_all(&mut t);
        let ctx = eframe::egui::Context::default();
        super::super::theme::install(&ctx, super::super::ThemePref::Light);
        for _ in 0..10 {
            frame(&mut t, &ctx);
        }
        let mut ms: Vec<f64> = (0..120)
            .map(|_| frame(&mut t, &ctx).1.as_secs_f64() * 1e3)
            .collect();
        ms.sort_by(f64::total_cmp);
        let (p50, p95) = (ms[60], ms[114]);
        println!("tree: 40,401 rows, frame p50 {p50:.3} ms p95 {p95:.3} ms");
        assert!(p95 < 4.0, "p95 {p95:.2} ms");
    }

    #[test]
    fn the_topology_tree_flattens_in_preorder() {
        let topo = Topology::parse(
            br#"{"format": "stepv-topology", "version": 1, "units": "mm",
              "tree": [{"name": "bracket-assy", "children": [
                {"name": "plate", "part": 0},
                {"name": "pins", "children": [{"name": "pin", "part": 1}, {"name": "pin", "part": 2}]}
              ]}],
              "parts": [
                {"name": "plate", "prototype": 0, "transform": [1,0,0,0, 0,1,0,0, 0,0,1,0]},
                {"name": "pin", "prototype": 0, "transform": [1,0,0,0, 0,1,0,0, 0,0,1,0]},
                {"name": "pin", "prototype": 0, "transform": [1,0,0,0, 0,1,0,0, 0,0,1,0]}
              ],
              "prototypes": [{"area": 1, "volume": null, "bbox": null, "vertices": [], "edges": [], "faces": []}]
            }"#,
        )
        .unwrap();
        let t = Tree::from_topology(&topo, 3);
        let r: Vec<_> = t
            .rows()
            .iter()
            .map(|r| (r.name.as_str(), r.depth, r.part, r.end))
            .collect();
        assert_eq!(
            r,
            [
                ("bracket-assy", 0, None, 5),
                ("plate", 1, Some(0), 2),
                ("pins", 1, None, 5),
                ("pin", 2, Some(1), 4),
                ("pin", 2, Some(2), 5),
            ]
        );
    }
}
