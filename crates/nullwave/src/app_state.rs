//! `AppState`: the one GPUI entity that glues the project model, undo history,
//! dirty tracking, workspace layout, settings and the audio engine together.
//! Views read it and call its methods; it notifies them on every change.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use basedrop::{Collector, Owned, Shared};
use gpui::{Context, PathPromptOptions, Subscription, Task};
use nw_core::{
    DocPath, Edit, History, Instrument, InstrumentId, Pattern, PatternId, Project,
    ProjectWatcher, Tick, clear_recovery, external_changes, hash_bytes, load_project,
    new_instrument, next_pattern, read_recovery, remove_instrument_edit, save_project,
    write_recovery,
};
use nw_engine::core::{Command, Feedback};
use nw_engine::handle::EngineHandle;
use nw_engine::sample::{SampleBuffer, SampleCache, load_sample, peaks};
use nw_engine::snapshot::compile_snapshot;

use crate::ui::modal::{Modal, ModalAction, Next};
use nw_workspace::{
    AUDIO_EXTENSIONS, AppSettings, Binding, DirtyTracker, TabKind, WorkspaceState, is_audio,
    load_settings, load_workspace, save_settings, save_workspace, settings_dir,
};

const POLL_INTERVAL: Duration = Duration::from_millis(16);
const WORKSPACE_SAVE_DELAY: Duration = Duration::from_millis(500);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);
/// Waveform resolution: min/max pairs per sample, whatever the view width.
const PEAK_BUCKETS: usize = 2048;

/// Peaks of an instrument's processed sample, for the waveform view.
pub struct Waveform {
    pub peaks: Arc<Vec<(f32, f32)>>,
    /// Address of the processed buffer they were computed from.
    source: usize,
}

fn buffer_id(buffer: &Shared<SampleBuffer>) -> usize {
    &**buffer as *const SampleBuffer as usize
}
pub const MIN_TEMPO: f64 = 20.0;
pub const MAX_TEMPO: f64 = 999.0;

#[derive(Clone, Debug, PartialEq)]
pub enum SampleStatus {
    NoSample,
    Ok,
    Missing(String),
}

pub struct AppState {
    /// Canonical path of the open project, `None` when no project is open.
    pub project_dir: Option<PathBuf>,
    pub project: Project,
    pub history: History,
    pub dirty: DirtyTracker,
    pub workspace: WorkspaceState,
    pub settings: AppSettings,
    // `samples` is declared before `engine` so cached buffers drop before the collector.
    pub samples: SampleCache,
    pub engine: Option<EngineHandle>,
    pub playhead: Tick,
    pub playing: bool,
    pub cpu_load: f32,
    pub log: Vec<String>,
    /// Content hash of every project file as last read or written, keyed by canonical path.
    pub last_saved: HashMap<PathBuf, u64>,
    /// The pattern `play_stop` last started; played when the focused tab is not a pattern.
    pub last_played_pattern: Option<PatternId>,
    settings_path: PathBuf,
    /// Owns sample memory when there is no engine (and so no engine collector).
    fallback_collector: Option<Collector>,
    /// Sample paths whose load failure is already in the log.
    reported_sample_errors: HashSet<PathBuf>,
    /// Bumped on every preview request so only the latest one sounds.
    preview_generation: u64,
    workspace_save: Option<Task<()>>,
    /// In-window prompts waiting to be answered; the front one is shown.
    pub modals: VecDeque<Modal>,
    /// Waveform peaks per instrument, computed in the background after each compile.
    pub waveforms: HashMap<InstrumentId, Waveform>,
    /// Each instrument's processed sample from the last compile.
    processed: HashMap<InstrumentId, Shared<SampleBuffer>>,
    /// Buffers whose peaks are being computed.
    peak_jobs: HashSet<usize>,
    peaks_stale: bool,
    /// The key bindings in effect, for showing shortcuts in the palette.
    pub bindings: Vec<Binding>,
    /// Watches the open project folder; batches of changed paths arrive on `watch_rx`.
    watcher: Option<ProjectWatcher>,
    watch_rx: Option<mpsc::Receiver<Vec<PathBuf>>>,
    /// Set when writing the recovery file failed, so the log is not flooded every 30 s.
    recovery_failed: bool,
    _recovery: Task<()>,
    _poll: Task<()>,
    _quit: Subscription,
}

impl AppState {
    /// Loads settings, starts the engine and reopens the last project if it still loads.
    pub fn new(cx: &mut Context<Self>) -> AppState {
        let mut log = Vec::new();
        let settings_path = settings_dir().join("settings.ron");
        let settings = read_settings(&settings_path, &mut log);

        let engine = match EngineHandle::start() {
            Ok(engine) => {
                let buffer = engine
                    .buffer_size
                    .map_or_else(|| "device default buffer".to_string(), |b| format!("buffer {b}"));
                log.push(format!("Audio output started: {} Hz, {buffer}", engine.sample_rate));
                Some(engine)
            }
            Err(e) => {
                log.push(format!("No audio output: {e}"));
                None
            }
        };
        let (fallback_collector, handle) = match &engine {
            Some(engine) => (None, engine.collector_handle()),
            None => {
                let collector = Collector::new();
                let handle = collector.handle();
                (Some(collector), handle)
            }
        };

        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this.update(cx, |state, cx| state.poll_engine(cx)).is_err() {
                    break;
                }
            }
        });
        let recovery = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(RECOVERY_INTERVAL).await;
                if this.update(cx, |state, _| state.write_recovery_if_dirty()).is_err() {
                    break;
                }
            }
        });
        let quit = cx.on_app_quit(|state, _cx| {
            state.save_workspace_now();
            async {}
        });

        let mut state = AppState {
            project_dir: None,
            project: Project::default(),
            history: History::new(),
            dirty: DirtyTracker::default(),
            workspace: WorkspaceState::default(),
            settings,
            samples: SampleCache::new(handle),
            engine,
            playhead: 0,
            playing: false,
            cpu_load: 0.0,
            log: Vec::new(),
            last_saved: HashMap::new(),
            last_played_pattern: None,
            settings_path,
            fallback_collector,
            reported_sample_errors: HashSet::new(),
            preview_generation: 0,
            workspace_save: None,
            modals: VecDeque::new(),
            waveforms: HashMap::new(),
            processed: HashMap::new(),
            peak_jobs: HashSet::new(),
            peaks_stale: false,
            bindings: Vec::new(),
            watcher: None,
            watch_rx: None,
            recovery_failed: false,
            _recovery: recovery,
            _poll: poll,
            _quit: quit,
        };
        for line in log {
            state.log(line);
        }
        if let Some(path) = state.settings.last_project.clone()
            && let Err(e) = state.open_project(&path)
        {
            state.log(format!("Could not reopen the last project: {e}"));
        }
        state.resync_engine();
        state
    }

    /// Adds a line to the Log panel (and mirrors it to stderr for terminal runs).
    pub fn log(&mut self, line: impl Into<String>) {
        let line = line.into();
        eprintln!("nullwave: {line}");
        self.log.push(line);
    }

    /// Replaces the current project with the one in `dir`, starts watching it, offers any
    /// crash recovery and remembers it as the last project. On error nothing changes.
    pub fn open_project(&mut self, dir: &Path) -> Result<(), String> {
        let dir = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let (project, warnings) = load_project(&dir).map_err(|e| e.to_string())?;
        // Leave the old project cleanly: its layout is saved and playback stops.
        self.save_workspace_now();
        if self.playing {
            self.send(Command::Stop);
            self.playing = false;
            self.playhead = 0;
        }
        self.last_saved = hash_project_files(&dir, &project);
        self.workspace = load_workspace(&dir, &project);
        self.project = project;
        self.history.clear();
        self.dirty.clear();
        self.last_played_pattern = None;
        self.log(format!("Opened project {}", dir.display()));
        for w in warnings {
            self.log(format!("Warning: {w:?}"));
        }
        self.start_watching(&dir);
        if let Some(recovered) = read_recovery(&dir) {
            self.modals.push_back(Modal::recovery(recovered));
        }
        if self.settings.last_project.as_deref() != Some(dir.as_path()) {
            self.settings.last_project = Some(dir.clone());
            self.save_settings();
        }
        self.project_dir = Some(dir);
        self.resync_engine();
        Ok(())
    }

    /// `open_project`, showing a failure as an in-window prompt.
    fn open_or_prompt(&mut self, dir: &Path, cx: &mut Context<Self>) {
        if let Err(e) = self.open_project(dir) {
            self.log(format!("Could not open project: {e}"));
            self.modals.push_back(Modal::error("Could not open project", e));
        }
        cx.notify();
    }

    fn start_watching(&mut self, dir: &Path) {
        let (tx, rx) = mpsc::channel();
        match ProjectWatcher::new(dir, move |paths| {
            let _ = tx.send(paths);
        }) {
            Ok(watcher) => {
                self.watcher = Some(watcher);
                self.watch_rx = Some(rx);
            }
            Err(e) => {
                self.watcher = None;
                self.watch_rx = None;
                self.log(format!("Not watching the project for outside changes: {e}"));
            }
        }
    }

    /// Writes every project file. On failure shows a prompt and leaves the project dirty.
    pub fn save(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(dir) = self.project_dir.clone() else { return false };
        let ok = match save_project(&dir, &self.project) {
            Ok(report) => {
                self.last_saved = report.written;
                record_gitignore(&dir, &mut self.last_saved);
                self.dirty.clear();
                if let Err(e) = clear_recovery(&dir) {
                    self.log(format!("Could not remove the recovery file: {e}"));
                }
                self.recovery_failed = false;
                self.log("Saved");
                true
            }
            Err(e) => {
                self.log(format!("Save failed: {e}"));
                self.modals.push_back(Modal::error("Could not save the project", e.to_string()));
                false
            }
        };
        cx.notify();
        ok
    }

    fn write_recovery_if_dirty(&mut self) {
        let Some(dir) = &self.project_dir else { return };
        if !self.dirty.is_dirty() {
            return;
        }
        match write_recovery(dir, &self.project) {
            Ok(()) => self.recovery_failed = false,
            Err(e) if !self.recovery_failed => {
                self.recovery_failed = true;
                self.log(format!("Could not write the recovery file: {e}"));
            }
            Err(_) => {}
        }
    }

    /// Marks every document of the project dirty, so the next save rewrites them all.
    fn mark_all_dirty(&mut self) {
        let docs = project_docs(&self.project);
        self.dirty.mark(docs);
    }

    /// Runs `next` now, or first asks to save when there are unsaved changes.
    fn guard_unsaved(&mut self, next: Next, cx: &mut Context<Self>) {
        if self.project_dir.is_some() && self.dirty.is_dirty() {
            self.modals.push_back(Modal::unsaved(next));
            cx.notify();
        } else {
            self.run_next(next, cx);
        }
    }

    fn run_next(&mut self, next: Next, cx: &mut Context<Self>) {
        match next {
            Next::Quit => self.finish_quit(cx),
            Next::Open(dir) => self.open_or_prompt(&dir, cx),
            Next::Create(dir) => self.create_project(&dir, cx),
        }
    }

    /// `app.quit` and closing the window: asks first when there are unsaved changes.
    pub fn request_quit(&mut self, cx: &mut Context<Self>) {
        self.guard_unsaved(Next::Quit, cx);
    }

    fn finish_quit(&mut self, cx: &mut Context<Self>) {
        if let Some(dir) = &self.project_dir {
            // Quitting without saving discards the changes, recovery included.
            if let Err(e) = clear_recovery(dir) {
                eprintln!("nullwave: could not remove the recovery file: {e}");
            }
        }
        self.save_workspace_now();
        cx.quit();
    }

    /// Asks for a project folder and opens it.
    pub fn request_open(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Project".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            this.update(cx, |state, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(dir) = paths.into_iter().next() {
                        state.guard_unsaved(Next::Open(dir), cx);
                    }
                }
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(e)) => {
                    state.log(format!("Could not open the folder picker: {e}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Asks for a new folder name with the native save panel and creates a project there.
    pub fn request_new(&mut self, cx: &mut Context<Self>) {
        let start = self
            .project_dir
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(dirs_home)
            .unwrap_or_else(|| PathBuf::from("/"));
        let path = cx.prompt_for_new_path(&start, Some("Untitled"));
        cx.spawn(async move |this, cx| {
            let result = path.await;
            this.update(cx, |state, cx| match result {
                Ok(Ok(Some(dir))) => state.guard_unsaved(Next::Create(dir), cx),
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(e)) => {
                    state.log(format!("Could not open the save panel: {e}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Creates `dir` holding a project with one empty "pattern 1", saves and opens it.
    fn create_project(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let not_empty = std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some());
        if dir.is_file() || not_empty {
            let message = format!("{} already exists and is not empty.", dir.display());
            self.modals.push_back(Modal::error("Could not create project", message));
            cx.notify();
            return;
        }
        let mut project = Project::default();
        let (id, name) = next_pattern(&project);
        project.patterns.push(Pattern { id, name, notes: BTreeMap::new() });
        if let Err(e) = save_project(dir, &project) {
            self.modals.push_back(Modal::error("Could not create project", e.to_string()));
            cx.notify();
            return;
        }
        self.open_or_prompt(dir, cx);
    }

    /// Answers the front prompt: `Some(button)` presses it, `None` (Esc) just closes it.
    pub fn resolve_modal(&mut self, button: Option<usize>, cx: &mut Context<Self>) {
        let Some(modal) = self.modals.pop_front() else { return };
        let action = button.and_then(|i| modal.buttons.get(i)).map(|b| b.action.clone());
        match action {
            None | Some(ModalAction::Dismiss) => {}
            Some(ModalAction::SaveThen(next)) => {
                if self.save(cx) {
                    self.run_next(next, cx);
                }
            }
            Some(ModalAction::DiscardThen(next)) => {
                // Opening another project replaces this one; nothing is written.
                self.dirty.clear();
                self.run_next(next, cx);
            }
            Some(ModalAction::Restore(project)) => {
                self.project = *project;
                self.history.clear();
                self.mark_all_dirty();
                self.after_load_change(cx);
                self.log("Restored unsaved changes from the last session");
            }
            Some(ModalAction::DiscardRecovery) => {
                if let Some(dir) = &self.project_dir
                    && let Err(e) = clear_recovery(dir)
                {
                    self.log(format!("Could not remove the recovery file: {e}"));
                }
            }
            Some(ModalAction::ReloadFromDisk) => self.reload_from_disk(cx),
            Some(ModalAction::KeepMine) => {
                self.mark_all_dirty();
                self.log("Kept your version; saving will overwrite the files on disk");
            }
        }
        cx.notify();
    }

    /// Handles a batch of changed paths from the watcher.
    fn on_outside_changes(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(dir) = self.project_dir.clone() else { return };
        let changed = external_changes(&paths, &self.last_saved, |p| std::fs::read(p).ok());
        if changed.is_empty() {
            return;
        }
        if !self.dirty.is_dirty() {
            self.reload_from_disk(cx);
            return;
        }
        // One prompt at a time for the same question.
        if self.modals.iter().any(|m| m.buttons.iter().any(|b| matches!(b.action, ModalAction::ReloadFromDisk))) {
            return;
        }
        let files: Vec<String> = changed
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap_or(p).display().to_string())
            .collect();
        self.modals.push_back(Modal::outside_changes(&files));
        cx.notify();
    }

    /// Replaces the in-memory project with the files on disk. A parse error keeps memory.
    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.project_dir.clone() else { return };
        match load_project(&dir) {
            Ok((project, _)) => {
                self.last_saved = hash_project_files(&dir, &project);
                self.project = project;
                // Undo steps refer to the old content; they no longer apply.
                self.history.clear();
                self.dirty.clear();
                self.after_load_change(cx);
                self.log("Reloaded the project from disk");
            }
            Err(e) => {
                self.log(format!("Could not reload the project: {e}"));
                self.modals.push_back(Modal::error("Could not reload the project", e.to_string()));
                cx.notify();
            }
        }
    }

    /// After the whole project was replaced: drop stale tabs and resync the engine.
    fn after_load_change(&mut self, cx: &mut Context<Self>) {
        let before = self.workspace.clone();
        self.workspace.retain_valid(&self.project);
        if self.workspace != before {
            self.workspace_changed(cx);
        }
        if let Some(p) = &self.last_played_pattern
            && self.project.pattern(p).is_none()
        {
            self.last_played_pattern = None;
        }
        self.resync_engine();
        cx.notify();
    }

    /// Commits one undo step, marks its documents dirty and updates the engine.
    pub fn apply(&mut self, edits: Vec<Edit>, cx: &mut Context<Self>) {
        let touched = self.history.commit(&mut self.project, edits);
        if !touched.is_empty() {
            self.after_change(touched, cx);
        }
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(touched) = self.history.undo(&mut self.project) {
            self.after_change(touched, cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(touched) = self.history.redo(&mut self.project) {
            self.after_change(touched, cx);
        }
    }

    fn after_change(&mut self, touched: Vec<DocPath>, cx: &mut Context<Self>) {
        self.dirty.mark(touched);
        let before = self.workspace.clone();
        self.workspace.retain_valid(&self.project);
        if self.workspace != before {
            self.workspace_changed(cx);
        }
        if let Some(p) = &self.last_played_pattern
            && self.project.pattern(p).is_none() {
                self.last_played_pattern = None;
            }
        self.resync_engine();
        cx.notify();
    }

    /// Opens `tab`, or focuses it if it is already open in any pane.
    pub fn open_tab(&mut self, tab: TabKind, cx: &mut Context<Self>) {
        self.update_workspace(cx, |ws| ws.open(tab));
    }

    /// Changes the workspace layout, then saves it (debounced) and re-renders.
    pub fn update_workspace(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut WorkspaceState)) {
        f(&mut self.workspace);
        self.workspace_changed(cx);
    }

    fn workspace_changed(&mut self, cx: &mut Context<Self>) {
        // Replacing the task cancels the pending save, which debounces bursts (e.g. resizing).
        self.workspace_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WORKSPACE_SAVE_DELAY).await;
            this.update(cx, |state, _| state.save_workspace_now()).ok();
        }));
        cx.notify();
    }

    pub fn save_workspace_now(&mut self) {
        let Some(dir) = &self.project_dir else { return };
        if let Err(e) = save_workspace(dir, &self.workspace) {
            self.log(format!("Could not save the workspace layout: {e}"));
        }
    }

    pub fn play_stop(&mut self, cx: &mut Context<Self>) {
        if self.engine.is_none() {
            self.log("Can't play: no audio output");
            cx.notify();
            return;
        }
        if self.playing {
            self.send(Command::Stop);
            self.playing = false;
            self.playhead = 0;
        } else {
            let pattern = self.workspace.active_pattern().or_else(|| self.last_played_pattern.clone());
            if pattern.is_none() {
                self.log("Nothing to play: open a pattern first");
                cx.notify();
                return;
            }
            self.last_played_pattern = pattern;
            self.recheck_samples();
            self.playing = true;
            self.resync_engine();
            self.send(Command::Play);
        }
        cx.notify();
    }

    /// Rebuilds the engine snapshot from the project and sends it.
    pub fn resync_engine(&mut self) {
        let pattern = self.snapshot_pattern();
        let snapshot = compile_snapshot(&self.project, pattern.as_ref(), &mut self.samples);
        self.processed = self
            .project
            .instruments
            .iter()
            .zip(&snapshot.instruments)
            .filter_map(|(inst, snap)| snap.sample.clone().map(|b| (inst.id.clone(), b)))
            .collect();
        self.peaks_stale = true;
        self.report_sample_errors();
        if let Some(engine) = &self.engine {
            let owned = Owned::new(&engine.collector_handle(), snapshot);
            self.send(Command::SetSnapshot(owned));
        }
    }

    /// The pattern the engine should hold: the playing one, else the one Play would start.
    fn snapshot_pattern(&self) -> Option<PatternId> {
        if self.playing {
            self.last_played_pattern.clone()
        } else {
            self.workspace.active_pattern().or_else(|| self.last_played_pattern.clone())
        }
    }

    /// The pattern shown in the status bar.
    pub fn status_pattern(&self) -> Option<&Pattern> {
        self.snapshot_pattern().and_then(|id| self.project.pattern(&id))
    }

    fn send(&mut self, command: Command) {
        let Some(engine) = &mut self.engine else { return };
        if engine.send(command).is_err() {
            self.log("Audio engine queue is full; a command was dropped");
        }
    }

    /// Before playback starts, retry failed samples (a drive may be back) and drop cached
    /// samples whose file has gone (renamed or unplugged), so playback matches the disk.
    fn recheck_samples(&mut self) {
        for inst in &self.project.instruments {
            let Some(path) = &inst.sampler.sample_path else { continue };
            match self.samples.status(path) {
                Some(Ok(())) if !path.exists() => self.samples.invalidate(path),
                Some(Err(_)) => self.samples.invalidate(path),
                _ => {}
            }
        }
    }

    fn report_sample_errors(&mut self) {
        let mut lines = Vec::new();
        for inst in &self.project.instruments {
            let Some(path) = &inst.sampler.sample_path else { continue };
            match self.samples.status(path) {
                Some(Err(e)) => {
                    if self.reported_sample_errors.insert(path.clone()) {
                        lines.push(format!("Instrument \"{}\" is silent: {e}", inst.name));
                    }
                }
                Some(Ok(())) => {
                    self.reported_sample_errors.remove(path);
                }
                None => {}
            }
        }
        for line in lines {
            self.log(line);
        }
    }

    pub fn sample_status(&self, id: &InstrumentId) -> SampleStatus {
        let Some(inst) = self.project.instrument(id) else { return SampleStatus::NoSample };
        let Some(path) = &inst.sampler.sample_path else { return SampleStatus::NoSample };
        match self.samples.status(path) {
            Some(Err(e)) => SampleStatus::Missing(e.to_string()),
            // Not requested yet: every resync loads all samples, so this is transient.
            Some(Ok(())) | None => SampleStatus::Ok,
        }
    }

    /// Starts background peak computations for processed samples that changed.
    fn refresh_waveforms(&mut self, cx: &mut Context<Self>) {
        self.peaks_stale = false;
        let processed = &self.processed;
        self.waveforms.retain(|id, _| processed.contains_key(id));
        let mut started = Vec::new();
        for (id, buffer) in &self.processed {
            let source = buffer_id(buffer);
            let current = self.waveforms.get(id).is_some_and(|w| w.source == source);
            if current || self.peak_jobs.contains(&source) {
                continue;
            }
            started.push(source);
            let (id, buffer) = (id.clone(), buffer.clone());
            let job = cx.background_executor().spawn(async move { peaks(&buffer, PEAK_BUCKETS) });
            cx.spawn(async move |this, cx| {
                let peaks = Arc::new(job.await);
                this.update(cx, |state, cx| {
                    state.peak_jobs.remove(&source);
                    // Only keep them if the instrument still plays that buffer.
                    if state.processed.get(&id).is_some_and(|b| buffer_id(b) == source) {
                        state.waveforms.insert(id, Waveform { peaks, source });
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        self.peak_jobs.extend(started);
    }

    fn poll_engine(&mut self, cx: &mut Context<Self>) {
        if self.peaks_stale {
            self.refresh_waveforms(cx);
        }
        let mut batches = Vec::new();
        if let Some(rx) = &self.watch_rx {
            while let Ok(paths) = rx.try_recv() {
                batches.extend(paths);
            }
        }
        if !batches.is_empty() {
            self.on_outside_changes(batches, cx);
        }
        let mut changed = false;
        if let Some(engine) = &mut self.engine {
            for feedback in engine.poll() {
                match feedback {
                    Feedback::Playhead { tick, playing } => {
                        changed |= tick != self.playhead || playing != self.playing;
                        self.playhead = tick;
                        self.playing = playing;
                    }
                    Feedback::Load(load) => {
                        changed |= (load - self.cpu_load).abs() >= 0.001;
                        self.cpu_load = load;
                    }
                }
            }
            engine.collect_garbage();
        }
        if let Some(collector) = &mut self.fallback_collector {
            collector.collect();
        }
        if changed {
            cx.notify();
        }
    }

    /// Commits `Edit::SetTempo`, clamped to the range the loader accepts.
    pub fn set_tempo(&mut self, tempo: f64, cx: &mut Context<Self>) {
        if !tempo.is_finite() {
            return;
        }
        let after = tempo.clamp(MIN_TEMPO, MAX_TEMPO);
        let before = self.project.tempo;
        if after != before {
            self.apply(vec![Edit::SetTempo { before, after }], cx);
        }
    }

    /// Adds the next numbered pattern, opens its tab and returns its id.
    pub fn new_pattern(&mut self, cx: &mut Context<Self>) -> Option<PatternId> {
        if self.project_dir.is_none() {
            self.log("Open or create a project first");
            cx.notify();
            return None;
        }
        let (id, name) = next_pattern(&self.project);
        let pattern = Pattern { id: id.clone(), name, notes: BTreeMap::new() };
        let index = self.project.patterns.len();
        self.apply(vec![Edit::AddPattern { pattern, index }], cx);
        self.open_tab(TabKind::Pattern(id.clone()), cx);
        Some(id)
    }

    /// Asks for a folder with the native folder picker and adds it to the sample folders.
    pub fn add_sample_folder(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add Folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            this.update(cx, |state, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        for path in paths {
                            state.add_folder(path);
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(e)) => state.log(format!("Could not open the folder picker: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Adds a sampler named after the file stem, playing `path`, at the end of the rack.
    pub fn add_instrument_from_sample(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.project_dir.is_none() {
            return;
        }
        let stem = path.file_stem().map_or_else(|| "sampler".into(), |s| s.to_string_lossy().into_owned());
        let (id, name) = new_instrument(&self.project, &stem);
        let mut instrument = Instrument::new(id, name);
        instrument.sampler.sample_path = Some(path);
        let index = self.project.instruments.len();
        self.apply(vec![Edit::AddInstrument { instrument: Box::new(instrument), index }], cx);
    }

    /// Points instrument `id` at a different sample file.
    pub fn set_instrument_sample(&mut self, id: &InstrumentId, path: PathBuf, cx: &mut Context<Self>) {
        let Some(before) = self.project.instrument(id).cloned() else { return };
        let mut after = before.clone();
        after.sampler.sample_path = Some(path.clone());
        // A file that was missing before may be back; load it fresh.
        self.samples.invalidate(&path);
        self.update_instrument(before, after, cx);
    }

    /// Commits one `UpdateInstrument` step (nothing if unchanged).
    pub fn update_instrument(&mut self, before: Instrument, after: Instrument, cx: &mut Context<Self>) {
        if before != after {
            self.apply(vec![Edit::UpdateInstrument { before: Box::new(before), after: Box::new(after) }], cx);
        }
    }

    /// Removes the instrument with all its notes; its tabs close. Undo restores both.
    pub fn delete_instrument(&mut self, id: &InstrumentId, cx: &mut Context<Self>) {
        if let Some(edit) = remove_instrument_edit(&self.project, id) {
            self.apply(vec![edit], cx);
        }
    }

    pub fn rename_pattern(&mut self, id: &PatternId, name: &str, cx: &mut Context<Self>) {
        let after = name.trim().to_string();
        let Some(pattern) = self.project.pattern(id) else { return };
        if after.is_empty() || after == pattern.name {
            return;
        }
        let before = pattern.name.clone();
        self.apply(vec![Edit::RenamePattern { id: id.clone(), before, after }], cx);
    }

    /// Removes the pattern; its tabs close. Undo restores it.
    pub fn delete_pattern(&mut self, id: &PatternId, cx: &mut Context<Self>) {
        let Some(index) = self.project.patterns.iter().position(|p| &p.id == id) else { return };
        let pattern = self.project.patterns[index].clone();
        if self.playing && self.last_played_pattern.as_ref() == Some(id) {
            self.send(Command::Stop);
            self.playing = false;
            self.playhead = 0;
        }
        self.apply(vec![Edit::RemovePattern { pattern, index }], cx);
    }

    /// Asks for audio files with the native picker and adds a sampler for each.
    pub fn prompt_add_instruments(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add Instrument".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            this.update(cx, |state, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        for path in paths {
                            if is_audio(&path) {
                                state.add_instrument_from_sample(path, cx);
                            } else {
                                let list = AUDIO_EXTENSIONS.join(", ");
                                state.log(format!("Not an audio file ({list}): {}", path.display()));
                            }
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(e)) => state.log(format!("Could not open the file picker: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks for one audio file with the native picker and loads it into instrument `id`.
    pub fn prompt_change_sample(&mut self, id: &InstrumentId, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Sample".into()),
        });
        let id = id.clone();
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            this.update(cx, |state, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            if is_audio(&path) {
                                state.set_instrument_sample(&id, path, cx);
                            } else {
                                let list = AUDIO_EXTENSIONS.join(", ");
                                state.log(format!("Not an audio file ({list}): {}", path.display()));
                            }
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(e)) => state.log(format!("Could not open the file picker: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Sounds one note through instrument `id` (e.g. when placing or clicking a note).
    pub fn preview_note(&mut self, id: &InstrumentId, pitch: u8, velocity: u8, length: Tick) {
        let Some(instrument) = self.project.instrument_index(id) else { return };
        self.send(Command::PreviewNote { instrument, pitch, velocity, length });
    }

    /// Plays `path` once through the preview voice, replacing any preview already sounding.
    /// The file loads on the background executor; a later click wins over a slower load.
    pub fn preview_sample(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(engine) = &self.engine else { return };
        let handle = engine.collector_handle();
        self.preview_generation += 1;
        let generation = self.preview_generation;
        let load = cx.background_executor().spawn(async move {
            load_sample(&path).map(|b| Shared::new(&handle, b)).map_err(|e| (path, e))
        });
        cx.spawn(async move |this, cx| {
            let loaded = load.await;
            this.update(cx, |state, cx| {
                if state.preview_generation != generation {
                    return;
                }
                match loaded {
                    Ok(sample) => state.send(Command::Preview(sample)),
                    Err((path, e)) => {
                        state.log(format!("Can't preview {}: {e}", path.display()));
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn add_folder(&mut self, path: PathBuf) {
        if self.settings.sample_folders.contains(&path) {
            return;
        }
        self.settings.sample_folders.push(path);
        self.save_settings();
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = save_settings(&self.settings_path, &self.settings) {
            self.log(format!("Could not save settings to {}: {e}", self.settings_path.display()));
        }
    }
}

/// Like `load_settings`, but logs why a settings file that exists could not be used.
fn read_settings(path: &Path, log: &mut Vec<String>) -> AppSettings {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            if let Err(e) = ron::from_str::<AppSettings>(&text) {
                log.push(format!("Settings file {} is invalid ({e}); using defaults", path.display()));
            }
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            log.push(format!("Could not read settings file {}: {e}; using defaults", path.display()));
        }
        Err(_) => {}
    }
    load_settings(path)
}

/// Hashes of the project files as they are on disk now, for outside-change detection.
fn hash_project_files(dir: &Path, project: &Project) -> HashMap<PathBuf, u64> {
    let mut hashes = project_docs(project)
        .into_iter()
        .filter_map(|doc| {
            let path = dir.join(doc.relative_path());
            std::fs::read(&path).ok().map(|bytes| (path, hash_bytes(&bytes)))
        })
        .collect();
    record_gitignore(dir, &mut hashes);
    hashes
}

/// The watcher tracks the project's `.gitignore` too; remember its content so writing it
/// ourselves does not look like an outside change.
fn record_gitignore(dir: &Path, hashes: &mut HashMap<PathBuf, u64>) {
    let path = dir.join(".gitignore");
    if let Ok(bytes) = std::fs::read(&path) {
        hashes.insert(path, hash_bytes(&bytes));
    }
}

/// Every document the project has on disk: the song, each instrument and each note file.
fn project_docs(project: &Project) -> Vec<DocPath> {
    let mut docs = vec![DocPath::Song];
    docs.extend(project.instruments.iter().map(|i| DocPath::Instrument(i.id.clone())));
    for p in &project.patterns {
        docs.extend(p.notes.iter().filter(|(_, n)| !n.is_empty()).map(|(i, _)| DocPath::Notes(p.id.clone(), i.clone())));
    }
    docs
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
