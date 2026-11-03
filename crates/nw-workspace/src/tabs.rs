//! Pure-Rust workspace state: tabs, panes, sidebar/panel layout and persistence.

use nw_core::{InstrumentId, PatternId, Project};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TabKind {
    Pattern(PatternId),
    PianoRoll(PatternId, InstrumentId),
    Instrument(InstrumentId),
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Pane {
    pub tabs: Vec<TabKind>,
    pub active: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SidebarView {
    Project,
    Sounds,
    Search,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PanelTab {
    Editor,
    Log,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub panes: Vec<Pane>,
    pub focused_pane: usize,
    pub sidebar_visible: bool,
    pub sidebar_view: SidebarView,
    pub sidebar_width: f32,
    pub panel_visible: bool,
    pub panel_tab: PanelTab,
    pub panel_height: f32,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            panes: vec![Pane::default()],
            focused_pane: 0,
            sidebar_visible: true,
            sidebar_view: SidebarView::Project,
            sidebar_width: 240.0,
            panel_visible: true,
            panel_tab: PanelTab::Editor,
            panel_height: 180.0,
        }
    }
}

fn is_valid(tab: &TabKind, project: &Project) -> bool {
    match tab {
        TabKind::Pattern(p) => project.pattern(p).is_some(),
        TabKind::PianoRoll(p, i) => project.pattern(p).is_some() && project.instrument(i).is_some(),
        TabKind::Instrument(i) => project.instrument(i).is_some(),
    }
}

impl WorkspaceState {
    /// Restores the structural invariants: at least one pane, a valid focused pane,
    /// every `active` is `None` iff its pane is empty and otherwise in range, and
    /// empty panes are collapsed when the workspace is split.
    fn normalize(&mut self) {
        if self.panes.is_empty() {
            self.panes.push(Pane::default());
        }
        for pane in &mut self.panes {
            pane.active = match (pane.tabs.len(), pane.active) {
                (0, _) => None,
                (n, Some(a)) => Some(a.min(n - 1)),
                (_, None) => Some(0),
            };
        }
        let mut i = 0;
        while self.panes.len() > 1 && i < self.panes.len() {
            if self.panes[i].tabs.is_empty() {
                self.panes.remove(i);
                if self.focused_pane > i {
                    self.focused_pane -= 1;
                }
            } else {
                i += 1;
            }
        }
        if self.focused_pane >= self.panes.len() {
            self.focused_pane = if self.panes.len() == 1 { 0 } else { self.panes.len() - 1 };
        }
    }

    pub fn open(&mut self, tab: TabKind) {
        self.normalize();
        let n = self.panes.len();
        for k in 0..n {
            let pi = (self.focused_pane + k) % n;
            if let Some(ti) = self.panes[pi].tabs.iter().position(|t| *t == tab) {
                self.focused_pane = pi;
                self.panes[pi].active = Some(ti);
                return;
            }
        }
        let pane = &mut self.panes[self.focused_pane];
        let at = pane.active.map_or(0, |a| a + 1);
        pane.tabs.insert(at, tab);
        pane.active = Some(at);
    }

    pub fn close(&mut self, pane: usize, index: usize) {
        let Some(p) = self.panes.get_mut(pane) else { return };
        if index >= p.tabs.len() {
            return;
        }
        p.tabs.remove(index);
        p.active = match p.active {
            _ if p.tabs.is_empty() => None,
            Some(a) if a == index => Some(index.min(p.tabs.len() - 1)),
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
        self.normalize();
    }

    pub fn close_active(&mut self) {
        if let Some(pane) = self.panes.get(self.focused_pane) {
            if let Some(a) = pane.active {
                self.close(self.focused_pane, a);
            }
        }
    }

    fn step(&mut self, forward: bool) {
        let Some(pane) = self.panes.get_mut(self.focused_pane) else { return };
        let n = pane.tabs.len();
        if let (Some(a), true) = (pane.active, n > 0) {
            pane.active = Some(if forward { (a + 1) % n } else { (a + n - 1) % n });
        }
    }

    pub fn activate_next(&mut self) {
        self.step(true);
    }

    pub fn activate_prev(&mut self) {
        self.step(false);
    }

    pub fn active_tab(&self) -> Option<&TabKind> {
        let pane = self.panes.get(self.focused_pane)?;
        pane.tabs.get(pane.active?)
    }

    pub fn split_right(&mut self) {
        if self.panes.len() != 1 {
            return;
        }
        let Some(tab) = self.active_tab().cloned() else { return };
        self.panes.push(Pane { tabs: vec![tab], active: Some(0) });
        self.focused_pane = 1;
    }

    pub fn retain_valid(&mut self, project: &Project) {
        for pane in &mut self.panes {
            let old_active = pane.active;
            let mut kept = Vec::with_capacity(pane.tabs.len());
            let mut new_active = None;
            let mut before_active = 0;
            for (i, tab) in std::mem::take(&mut pane.tabs).into_iter().enumerate() {
                if is_valid(&tab, project) {
                    if Some(i) == old_active {
                        new_active = Some(kept.len());
                    }
                    if old_active.is_some_and(|a| i < a) {
                        before_active += 1;
                    }
                    kept.push(tab);
                }
            }
            // Active tab was dropped: fall to the tab now occupying its place.
            pane.active = new_active.or(Some(before_active));
            pane.tabs = kept;
        }
        self.normalize();
    }

    pub fn active_pattern(&self) -> Option<PatternId> {
        match self.active_tab()? {
            TabKind::Pattern(p) | TabKind::PianoRoll(p, _) => Some(p.clone()),
            TabKind::Instrument(_) => None,
        }
    }
}

pub fn tab_title(tab: &TabKind, project: &Project) -> String {
    let pattern = |id: &PatternId| project.pattern(id).map_or_else(|| id.0.clone(), |p| p.name.clone());
    let instrument =
        |id: &InstrumentId| project.instrument(id).map_or_else(|| id.0.clone(), |i| i.name.clone());
    match tab {
        TabKind::Pattern(p) => pattern(p),
        TabKind::PianoRoll(p, i) => format!("{} / {}", pattern(p), instrument(i)),
        TabKind::Instrument(i) => match project.instrument(i) {
            Some(inst) => format!("Sampler: {}", inst.name),
            None => i.0.clone(),
        },
    }
}

pub fn save_workspace(dir: &Path, state: &WorkspaceState) -> io::Result<()> {
    let folder = dir.join(".nullwave");
    std::fs::create_dir_all(&folder)?;
    let text = ron::ser::to_string_pretty(state, ron::ser::PrettyConfig::new())
        .map_err(io::Error::other)?;
    let tmp = folder.join("workspace.ron.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, folder.join("workspace.ron")).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

pub fn load_workspace(dir: &Path, project: &Project) -> WorkspaceState {
    let mut state = std::fs::read_to_string(dir.join(".nullwave/workspace.ron"))
        .ok()
        .and_then(|t| ron::from_str::<WorkspaceState>(&t).ok())
        .unwrap_or_default();
    state.normalize();
    state.retain_valid(project);
    state
}
