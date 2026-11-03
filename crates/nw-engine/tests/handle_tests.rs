use std::time::Duration;

use nw_engine::core::{Command, Feedback};
use nw_engine::handle::EngineHandle;

#[test]
#[ignore = "needs audio device"]
fn starts_and_reports_playhead() {
    let mut engine = EngineHandle::start().expect("engine starts on default device");
    assert!(engine.sample_rate > 0);
    assert!(engine.send(Command::Play).is_ok());
    std::thread::sleep(Duration::from_millis(300));
    let feedback = engine.poll();
    engine.collect_garbage();
    assert!(
        feedback
            .iter()
            .any(|f| matches!(f, Feedback::Playhead { tick, playing: true } if *tick > 0)),
        "no advancing playhead in {feedback:?}"
    );
}
