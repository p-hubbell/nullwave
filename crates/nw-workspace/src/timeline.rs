//! Timeline geometry for pattern and song tabs: lanes, blocks, pattern outlines, and what
//! a point lands on. Pure; the app paints and handles the mouse.

use nw_core::{
    AudioLaneId, AudioPlacement, ClipRef, Container, InstrumentId, PPQ, PatternPlacement, Placement, PlayTarget,
    Project, Tick, TimeSignature, Timeline,
};

/// Edge grab zone of a block, in pixels (never more than a third of the block).
pub const EDGE_PX: f32 = 6.0;
/// Height of a pattern outline's name tab, from the top of the first lane.
pub const LABEL_HEIGHT: f32 = 18.0;
/// Widest a name tab gets (it is never wider than its outline).
pub const LABEL_MAX_WIDTH: f32 = 120.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lane {
    Instrument(InstrumentId),
    Audio(AudioLaneId),
    /// The "+ add instrument" row.
    AddInstrument,
}

/// Instruments in project order; then, in the song, its audio lanes; then the add row.
pub fn lanes(p: &Project, c: &Container) -> Vec<Lane> {
    let mut out: Vec<Lane> = p.instruments.iter().map(|i| Lane::Instrument(i.id.clone())).collect();
    if *c == Container::Song {
        out.extend(p.song.audio_lanes.iter().map(|l| Lane::Audio(l.id.clone())));
    }
    out.push(Lane::AddInstrument);
    out
}

/// Whose block this is: the tab's own timeline, or a pattern placed in the song.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    Direct,
    Pattern(PatternPlacement),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// `placement` is in its owner's timeline (pattern-local ticks for `Owner::Pattern`).
    Clip { placement: Placement, owner: Owner },
    Audio(AudioPlacement),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub lane: usize,
    /// Timeline ticks of the tab.
    pub start: Tick,
    pub end: Tick,
    /// The clip number, or the audio file's name.
    pub label: String,
}

fn lane_of(p: &Project, instrument: &InstrumentId) -> Option<usize> {
    p.instrument_index(instrument)
}

fn clip_blocks(p: &Project, t: &Timeline, offset: Tick, owner: &Owner, out: &mut Vec<Block>) {
    for placement in &t.placements {
        let (Some(clip), Some(lane)) = (t.clip(&placement.clip), lane_of(p, &placement.clip.instrument)) else {
            continue;
        };
        let start = offset + placement.at;
        out.push(Block {
            kind: BlockKind::Clip { placement: placement.clone(), owner: owner.clone() },
            lane,
            start,
            end: start + clip.length,
            label: clip.key.number.to_string(),
        });
    }
}

/// Every block the tab shows. In the song: loose clips, then each placed pattern's clips
/// (in placement order), then audio.
pub fn blocks(p: &Project, c: &Container, audio_ticks: &dyn Fn(&AudioLaneId) -> Tick) -> Vec<Block> {
    let mut out = Vec::new();
    let Some(t) = p.timeline(c) else { return out };
    clip_blocks(p, t, 0, &Owner::Direct, &mut out);
    if *c == Container::Song {
        for placed in &p.song.patterns {
            if let Some(pattern) = p.pattern(&placed.pattern) {
                clip_blocks(p, &pattern.timeline, placed.at, &Owner::Pattern(placed.clone()), &mut out);
            }
        }
        let first_audio = p.instruments.len();
        for a in &p.song.audio {
            let Some(i) = p.song.audio_lanes.iter().position(|l| l.id == a.lane) else { continue };
            let name = p.song.audio_lanes[i].path.file_name().map(|n| n.to_string_lossy().into_owned());
            out.push(Block {
                kind: BlockKind::Audio(a.clone()),
                lane: first_audio + i,
                start: a.at,
                end: a.at + audio_ticks(&a.lane).max(1),
                label: name.unwrap_or_else(|| a.lane.0.clone()),
            });
        }
    }
    out
}

/// A placed pattern's frame in the song.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    pub placement: PatternPlacement,
    pub start: Tick,
    pub end: Tick,
    pub name: String,
}

pub fn outlines(p: &Project) -> Vec<Outline> {
    p.song
        .patterns
        .iter()
        .filter_map(|pp| {
            let pattern = p.pattern(&pp.pattern)?;
            Some(Outline {
                placement: pp.clone(),
                start: pp.at,
                end: pp.at + p.pattern_length(&pp.pattern),
                name: pattern.name.clone(),
            })
        })
        .collect()
}

/// Pixel mapping of a timeline tab. `scroll` is the tick at the left edge of the lanes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineView {
    pub px_per_tick: f32,
    pub scroll: f32,
    pub lane_height: f32,
    pub header_width: f32,
    pub ruler_height: f32,
}

impl TimelineView {
    pub fn x_of(&self, tick: Tick) -> f32 {
        self.header_width + (tick as f32 - self.scroll) * self.px_per_tick
    }

    pub fn tick_at(&self, x: f32) -> f32 {
        (x - self.header_width) / self.px_per_tick + self.scroll
    }

    /// The lane row under `y` (rows beyond the last lane included; `None` in the ruler).
    pub fn lane_at(&self, y: f32) -> Option<usize> {
        (y >= self.ruler_height).then(|| ((y - self.ruler_height) / self.lane_height) as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Body,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineHit {
    Ruler(Tick),
    Header(usize),
    Block { index: usize, edge: Edge },
    OutlineLabel(PatternPlacement),
    /// `inside` is the placed pattern whose outline contains the point (instrument lanes only).
    Empty { lane: usize, tick: Tick, inside: Option<PatternPlacement> },
    Nothing,
}

fn block_hit(view: &TimelineView, b: &Block, x: f32) -> Option<Edge> {
    let (x0, x1) = (view.x_of(b.start), view.x_of(b.end));
    if x < x0 || x >= x1 {
        return None;
    }
    let edge = EDGE_PX.min((x1 - x0) / 3.0);
    Some(if x < x0 + edge {
        Edge::Left
    } else if x >= x1 - edge {
        Edge::Right
    } else {
        Edge::Body
    })
}

/// What (`x`, `y`) lands on. Order: ruler, lane headers, the tab's own blocks (loose clips
/// and audio sit on top), outline name tabs, placed patterns' blocks, then empty space.
pub fn hit_test(view: &TimelineView, lanes: &[Lane], blocks: &[Block], outlines: &[Outline], x: f32, y: f32) -> TimelineHit {
    let tick = view.tick_at(x).max(0.0) as Tick;
    let Some(lane) = view.lane_at(y) else { return TimelineHit::Ruler(tick) };
    if lane >= lanes.len() {
        return TimelineHit::Nothing;
    }
    if x < view.header_width {
        return TimelineHit::Header(lane);
    }
    let direct = |b: &Block| !matches!(b.kind, BlockKind::Clip { owner: Owner::Pattern(_), .. });
    let find = |own: bool| {
        blocks.iter().enumerate().find_map(|(index, b)| {
            (b.lane == lane && direct(b) == own).then(|| block_hit(view, b, x)).flatten().map(|edge| (index, edge))
        })
    };
    if let Some((index, edge)) = find(true) {
        return TimelineHit::Block { index, edge };
    }
    let label_top = view.ruler_height;
    if (label_top..label_top + LABEL_HEIGHT).contains(&y) {
        for o in outlines {
            let x0 = view.x_of(o.start);
            let width = (view.x_of(o.end) - x0).min(LABEL_MAX_WIDTH);
            if (x0..x0 + width).contains(&x) {
                return TimelineHit::OutlineLabel(o.placement.clone());
            }
        }
    }
    if let Some((index, edge)) = find(false) {
        return TimelineHit::Block { index, edge };
    }
    let inside = match lanes[lane] {
        Lane::Instrument(_) => outlines.iter().find(|o| (o.start..o.end).contains(&tick)).map(|o| o.placement.clone()),
        _ => None,
    };
    TimelineHit::Empty { lane, tick, inside }
}

/// `tick` relative to a placed pattern starting at `outline_at`; `None` before its start.
pub fn to_pattern_local(outline_at: Tick, tick: Tick) -> Option<Tick> {
    tick.checked_sub(outline_at)
}

/// Where `clip`'s piano roll shows the playhead (clip-local ticks) while `playing` is at
/// `playhead`: inside whichever of its blocks is sounding, `None` if none is.
pub fn clip_playhead(p: &Project, playing: &PlayTarget, clip: &ClipRef, playhead: Tick) -> Option<Tick> {
    let in_timeline = |t: &Timeline, tick: Tick| {
        let c = t.clip(&clip.key)?;
        t.placements
            .iter()
            .filter(|pl| pl.clip == clip.key)
            .find(|pl| (pl.at..pl.at + c.length).contains(&tick))
            .map(|pl| tick - pl.at + c.start)
    };
    match (playing, &clip.container) {
        (PlayTarget::Pattern(a), Container::Pattern(b)) if a == b => in_timeline(&p.pattern(b)?.timeline, playhead),
        (PlayTarget::Song, Container::Song) => in_timeline(&p.song.timeline, playhead),
        (PlayTarget::Song, Container::Pattern(id)) => {
            let t = &p.pattern(id)?.timeline;
            p.song
                .patterns
                .iter()
                .filter(|pp| &pp.pattern == id && pp.at <= playhead)
                .find_map(|pp| in_timeline(t, playhead - pp.at))
        }
        _ => None,
    }
}

/// The timeline's snap menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SnapSize {
    #[default]
    Bar,
    Beat,
    Sixteenth,
    Off,
}

impl SnapSize {
    pub fn ticks(self, ts: TimeSignature) -> Tick {
        match self {
            SnapSize::Bar => ts.ticks_per_bar().max(1),
            SnapSize::Beat => (PPQ * 4 / u32::from(ts.denominator.max(1))).max(1),
            SnapSize::Sixteenth => PPQ / 4,
            SnapSize::Off => 1,
        }
    }

    /// The next size in the menu (wrapping).
    pub fn next(self) -> SnapSize {
        match self {
            SnapSize::Bar => SnapSize::Beat,
            SnapSize::Beat => SnapSize::Sixteenth,
            SnapSize::Sixteenth => SnapSize::Off,
            SnapSize::Off => SnapSize::Bar,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SnapSize::Bar => "Bar",
            SnapSize::Beat => "Beat",
            SnapSize::Sixteenth => "1/16",
            SnapSize::Off => "Off",
        }
    }
}

/// A drag distance in ticks rounded to the nearest multiple of `snap`.
pub fn snap_delta(ticks: f32, snap: Tick) -> i64 {
    let snap = snap.max(1) as f32;
    (ticks / snap).round() as i64 * snap as i64
}
