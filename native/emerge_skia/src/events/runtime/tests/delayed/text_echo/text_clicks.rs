use super::*;
use crate::input::ACTION_RELEASE;

fn button(action: u8, position: (f32, f32), mods: u8) -> InputEvent {
    InputEvent::CursorButton {
        button: "left".into(),
        action,
        mods,
        x: position.0,
        y: position.1,
    }
}

fn point(host: &HostEventRuntime, character: usize) -> (f32, f32) {
    let state = host.focused_text_state().unwrap();
    let layout = state.text_layout(&state.content);
    let line_index = layout.line_index_for_cursor(character);
    let line = &layout.lines[line_index];
    let x = line.offset_for_cursor(character);
    let next = line.offset_for_cursor(character + 1);
    (
        state.line_x(line.width) + x + (next - x) * 0.75,
        state.frame_y + state.inset_top + (line_index as f32 + 0.5) * layout.line_height,
    )
}

fn edge(host: &mut HostEventRuntime, action: u8, position: (f32, f32), at: Instant) {
    host.driver.handle_input_at(button(action, position, 0), at);
}

#[test]
fn double_click_selects_word_in_text_and_multiline_and_survives_release_and_echo() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for kind in [ElementKind::TextInput, ElementKind::Multiline] {
        let (mut engine, mut host, _) = setup(kind);
        commit(&mut engine, &mut host, "hello world");
        // Click the right half of the last glyph: nearest caret is at the space.
        let position = point(&host, 4);
        let now = Instant::now();
        for (index, action) in [ACTION_PRESS, ACTION_RELEASE, ACTION_PRESS]
            .into_iter()
            .enumerate()
        {
            edge(
                &mut host,
                action,
                position,
                now + Duration::from_millis(index as u64 * 80),
            );
            settle(&mut engine, &mut host);
        }
        assert_eq!(
            host.focused_text_state().unwrap().selected_range(),
            Some((0, 5))
        );
        host.handle_input(InputEvent::CursorPos {
            x: position.0 + 0.5,
            y: position.1,
        });
        edge(
            &mut host,
            ACTION_RELEASE,
            position,
            now + Duration::from_millis(200),
        );
        settle(&mut engine, &mut host);
        echo(&mut engine, &mut host, "hello world");
        assert_eq!(
            host.focused_text_state()
                .unwrap()
                .selection_text()
                .as_deref(),
            Some("hello")
        );
        assert_eq!(
            host.driver
                .runtime
                .clipboard
                .get_text(ClipboardTarget::Primary)
                .as_deref(),
            Some("hello")
        );
        assert!(host.handle_text_input_command(TextInputCommandRequest::Copy));
        assert_eq!(
            host.driver
                .runtime
                .clipboard
                .get_text(ClipboardTarget::Clipboard)
                .as_deref(),
            Some("hello")
        );
        commit(&mut engine, &mut host, "X");
        assert_text(&host, "X world", 1);
    }
}

#[test]
fn buffered_button_edges_use_occurrence_times_not_replay_times() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    for (delay_ms, expected) in [(150, Some((6, 11))), (501, None)] {
        let (mut engine, mut host, _) = setup(ElementKind::TextInput);
        commit(&mut engine, &mut host, "hello world");
        let position = point(&host, 7);
        let rebuild = response(
            &mut engine,
            vec![TreeMsg::RebuildRegistry],
            TreeUpdateDecodePolicy::ReturnErr,
        );
        host.driver.runtime.listener_lane.stale = true;
        let now = Instant::now() - Duration::from_secs(5);
        let (input_tx, input_rx) = bounded(4);
        for (action, delay) in [
            (ACTION_PRESS, 0),
            (ACTION_RELEASE, 10),
            (ACTION_PRESS, delay_ms),
            (ACTION_RELEASE, delay_ms + 10),
        ] {
            input_tx
                .send(EventMsg::TimedButton {
                    event: button(action, position, 0),
                    occurred_at: now + Duration::from_millis(delay),
                })
                .unwrap();
        }
        let mut pending = None;
        while let Ok(message) = input_rx.try_recv() {
            assert!(
                host.driver
                    .handle_actor_message(message, &input_rx, &mut pending)
            );
        }
        assert!(pending.is_none());
        assert_eq!(host.driver.runtime.listener_lane.buffered_inputs.len(), 4);
        host.install_rebuild(rebuild);
        settle(&mut engine, &mut host);
        assert_eq!(
            host.focused_text_state().unwrap().selected_range(),
            expected
        );
    }
}

#[test]
fn shift_click_drag_and_intervening_edits_keep_their_existing_semantics() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::TextInput);
    commit(&mut engine, &mut host, "hello world");
    let start = point(&host, 0);
    let end = point(&host, 3);
    let now = Instant::now();
    edge(&mut host, ACTION_PRESS, start, now);
    host.handle_input(InputEvent::CursorPos { x: end.0, y: end.1 });
    edge(
        &mut host,
        ACTION_RELEASE,
        end,
        now + Duration::from_millis(30),
    );
    settle(&mut engine, &mut host);
    let state = host.focused_text_state().unwrap();
    assert_eq!(state.selected_range(), Some((1, 4)));
    // An intervening edit cancels the click chain, even at the same position/time.
    commit(&mut engine, &mut host, "X");
    edge(
        &mut host,
        ACTION_PRESS,
        start,
        now + Duration::from_millis(100),
    );
    edge(
        &mut host,
        ACTION_RELEASE,
        start,
        now + Duration::from_millis(110),
    );
    settle(&mut engine, &mut host);
    assert_eq!(host.focused_text_state().unwrap().selected_range(), None);
    host.driver.handle_input_at(
        button(ACTION_PRESS, end, crate::input::MOD_SHIFT),
        now + Duration::from_millis(150),
    );
    settle(&mut engine, &mut host);
    assert!(
        host.focused_text_state()
            .unwrap()
            .selected_range()
            .is_some()
    );
}

#[test]
fn wrapped_multiline_word_selection_uses_pointer_line_and_unicode_boundaries() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (mut engine, mut host, _) = setup(ElementKind::Multiline);
    let text = "a fairly long prefix with wrapping cafe\u{301} noir";
    commit(&mut engine, &mut host, text);
    let character = text.find("cafe").unwrap() + 2;
    let state = host.focused_text_state().unwrap();
    assert!(state.text_layout(text).line_index_for_cursor(character) > 0);
    let position = point(&host, character);
    let now = Instant::now();
    for (index, action) in [ACTION_PRESS, ACTION_RELEASE, ACTION_PRESS, ACTION_RELEASE]
        .into_iter()
        .enumerate()
    {
        edge(
            &mut host,
            action,
            position,
            now + Duration::from_millis(index as u64 * 30),
        );
        settle(&mut engine, &mut host);
    }
    assert_eq!(
        host.focused_text_state()
            .unwrap()
            .selection_text()
            .as_deref(),
        Some("cafe\u{301}")
    );
}
