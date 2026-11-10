use nw_core::*;
use nw_workspace::timeline::*;

fn iid(s: &str) -> InstrumentId { InstrumentId(s.into()) }
fn pid(s: &str) -> PatternId { PatternId(s.into()) }
fn key(inst: &str, n: ClipNumber) -> ClipKey { ClipKey { instrument: iid(inst), number: n } }
fn pp(at: Tick) -> PatternPlacement { PatternPlacement { pattern: pid("p1"), at } }

/// p1: bell 1 (one bar) at 0 and kick 2 (half a bar) at 1920. Song: p1 at 0 and at 7680,
/// a loose bell 1 at 960, a tag on an audio lane at 0.
fn project() -> Project {
    let mut p = Project::default();
    p.instruments.push(Instrument::new(iid("bell"), "Bell"));
    p.instruments.push(Instrument::new(iid("kick"), "Kick"));
    let mut t = Timeline::default();
    t.clips.push(Clip::new(key("bell", 1), 3840));
    t.clips.push(Clip::new(key("kick", 2), 1920));
    t.placements = vec![Placement { clip: key("bell", 1), at: 0 }, Placement { clip: key("kick", 2), at: 1920 }];
    p.patterns.push(Pattern { id: pid("p1"), name: "Hook".into(), timeline: t });
    p.song.patterns = vec![pp(0), pp(7680)];
    p.song.timeline.clips.push(Clip::new(key("bell", 1), 480));
    p.song.timeline.placements.push(Placement { clip: key("bell", 1), at: 960 });
    p.song.audio_lanes.push(AudioLane { id: AudioLaneId("tag".into()), path: "/s/My Tag.wav".into(), volume: 1.0 });
    p.song.audio.push(AudioPlacement { lane: AudioLaneId("tag".into()), at: 0 });
    p
}

fn view() -> TimelineView {
    TimelineView { px_per_tick: 0.1, scroll: 0.0, lane_height: 40.0, header_width: 100.0, ruler_height: 20.0 }
}

fn audio_len(_: &AudioLaneId) -> Tick { 960 }

#[test]
fn lane_order() {
    let p = project();
    assert_eq!(
        lanes(&p, &Container::Pattern(pid("p1"))),
        vec![Lane::Instrument(iid("bell")), Lane::Instrument(iid("kick")), Lane::AddInstrument]
    );
    assert_eq!(
        lanes(&p, &Container::Song),
        vec![Lane::Instrument(iid("bell")), Lane::Instrument(iid("kick")), Lane::Audio(AudioLaneId("tag".into())), Lane::AddInstrument]
    );
}

#[test]
fn pattern_blocks() {
    let p = project();
    let b = blocks(&p, &Container::Pattern(pid("p1")), &audio_len);
    assert_eq!(b.len(), 2);
    assert_eq!((b[0].lane, b[0].start, b[0].end, b[0].label.as_str()), (0, 0, 3840, "1"));
    assert_eq!((b[1].lane, b[1].start, b[1].end, b[1].label.as_str()), (1, 1920, 3840, "2"));
    assert!(matches!(&b[0].kind, BlockKind::Clip { owner: Owner::Direct, .. }));
}

#[test]
fn song_blocks_expand_placed_patterns() {
    let p = project();
    let b = blocks(&p, &Container::Song, &audio_len);
    let pattern_blocks: Vec<(usize, Tick, Tick)> = b
        .iter()
        .filter(|x| matches!(&x.kind, BlockKind::Clip { owner: Owner::Pattern(_), .. }))
        .map(|x| (x.lane, x.start, x.end))
        .collect();
    assert_eq!(pattern_blocks, vec![(0, 0, 3840), (1, 1920, 3840), (0, 7680, 11520), (1, 9600, 11520)]);
    let second = b.iter().find(|x| x.start == 9600).unwrap();
    assert!(matches!(&second.kind, BlockKind::Clip { owner: Owner::Pattern(o), placement } if *o == pp(7680) && placement.at == 1920));
    let audio = b.iter().find(|x| matches!(x.kind, BlockKind::Audio(_))).unwrap();
    assert_eq!((audio.lane, audio.start, audio.end, audio.label.as_str()), (2, 0, 960, "My Tag.wav"));
    assert_eq!(outlines(&p), vec![
        Outline { placement: pp(0), start: 0, end: 3840, name: "Hook".into() },
        Outline { placement: pp(7680), start: 7680, end: 11520, name: "Hook".into() },
    ]);
}

#[test]
fn view_maths_round_trip() {
    for px in [0.01f32, 0.1, 0.5] {
        let v = TimelineView { px_per_tick: px, scroll: 480.0, ..view() };
        for t in [480u32, 960, 12345] {
            assert!((v.tick_at(v.x_of(t)) - t as f32).abs() < 1e-2);
        }
    }
    let v = view();
    assert_eq!(v.lane_at(10.0), None);
    assert_eq!(v.lane_at(20.0), Some(0));
    assert_eq!(v.lane_at(61.0), Some(1));
}

fn hit(p: &Project, c: &Container, x: f32, y: f32) -> TimelineHit {
    let l = lanes(p, c);
    let b = blocks(p, c, &audio_len);
    let o = if *c == Container::Song { outlines(p) } else { Vec::new() };
    hit_test(&view(), &l, &b, &o, x, y)
}

#[test]
fn hits_in_priority_order() {
    let p = project();
    let song = Container::Song;
    let blocks = blocks(&p, &song, &audio_len);
    let index_of = |pred: &dyn Fn(&Block) -> bool| blocks.iter().position(|b| pred(b)).unwrap();
    // Ruler and headers.
    assert_eq!(hit(&p, &song, 150.0, 5.0), TimelineHit::Ruler(500));
    assert_eq!(hit(&p, &song, 50.0, 70.0), TimelineHit::Header(1));
    // The loose bell (960..1440) wins over the pattern's bell 1 underneath it.
    let loose = index_of(&|b| matches!(&b.kind, BlockKind::Clip { owner: Owner::Direct, .. }));
    assert_eq!(hit(&p, &song, 100.0 + 120.0, 50.0), TimelineHit::Block { index: loose, edge: Edge::Body });
    // The outline's name tab (top 18 px of the first lane, left part).
    assert_eq!(hit(&p, &song, 100.0 + 770.0, 25.0), TimelineHit::OutlineLabel(pp(7680)));
    // Below the tab: the pattern's bell block.
    let bell_in_second = index_of(&|b| b.start == 7680);
    assert_eq!(hit(&p, &song, 100.0 + 800.0, 50.0), TimelineHit::Block { index: bell_in_second, edge: Edge::Body });
    // Empty space inside an outline reports it; outside it does not.
    assert_eq!(hit(&p, &song, 100.0 + 50.0, 70.0), TimelineHit::Empty { lane: 1, tick: 500, inside: Some(pp(0)) });
    assert_eq!(hit(&p, &song, 100.0 + 500.0, 70.0), TimelineHit::Empty { lane: 1, tick: 5000, inside: None });
    // Below the add row: nothing.
    assert_eq!(hit(&p, &song, 300.0, 500.0), TimelineHit::Nothing);
    // The add row is a lane but holds no blocks.
    assert_eq!(hit(&p, &song, 300.0, 150.0), TimelineHit::Empty { lane: 3, tick: 2000, inside: None });
}

#[test]
fn edges_stay_within_a_third() {
    let p = project();
    let c = Container::Pattern(pid("p1"));
    // kick 2: 1920..3840 → x 292..484 (192 px): 6 px edges.
    assert_eq!(hit(&p, &c, 293.0, 70.0), TimelineHit::Block { index: 1, edge: Edge::Left });
    assert_eq!(hit(&p, &c, 483.0, 70.0), TimelineHit::Block { index: 1, edge: Edge::Right });
    assert_eq!(hit(&p, &c, 300.0, 70.0), TimelineHit::Block { index: 1, edge: Edge::Body });
    // A 9 px block has 3 px edges.
    let tiny = TimelineView { px_per_tick: 9.0 / 3840.0, ..view() };
    let l = lanes(&p, &c);
    let b = blocks(&p, &c, &audio_len);
    assert_eq!(hit_test(&tiny, &l, &b, &[], 104.0, 30.0), TimelineHit::Block { index: 0, edge: Edge::Body });
    assert_eq!(hit_test(&tiny, &l, &b, &[], 101.0, 30.0), TimelineHit::Block { index: 0, edge: Edge::Left });
}

#[test]
fn pattern_local_ticks_clamp_at_outline_start() {
    assert_eq!(to_pattern_local(7680, 9600), Some(1920));
    assert_eq!(to_pattern_local(7680, 7680), Some(0));
    assert_eq!(to_pattern_local(7680, 7000), None);
}
