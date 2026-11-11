//! 触摸输入的集成测试：在实际布局上验证命中测试、滚动、连击和长按菜单。
//!
//! 离屏渲染的缩放比例为 `SCALE`（两倍），触摸坐标按物理像素传入。

use super::*;
use crate::app::input::context_menu::{MenuEntry, PopupRow};
use winit::event::{DeviceId, Touch, TouchPhase};

#[cfg(windows)]
mod native;

fn touch(app: &mut App, id: u64, phase: TouchPhase, x: f32, y: f32) {
    app.on_touch(Touch {
        device_id: DeviceId::dummy(),
        phase,
        location: PhysicalPosition::new(f64::from(x * SCALE), f64::from(y * SCALE)),
        force: None,
        id,
    });
}

fn document() -> App {
    let mut app = blank_app(false);
    app.new_file();
    app.run(Command::Insert("hello world\n".repeat(400)));
    app.editor.active_mut().unwrap().selection = winc_core::Selection::point(0);
    app.editor.active_mut().unwrap().scroll_line = 0.0;
    paint(&mut app);
    app
}

#[test]
fn touch_taps_position_the_caret_at_high_dpi_and_double_taps_select() {
    let mut app = document();
    let x = app.layout.text.x + TEXT_PAD + app.cell * 3.0;
    let y = app.layout.text.y + app.fonts.line_height / 2.0;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    assert_eq!(app.editor.active().unwrap().selection.head, 0);
    assert_eq!(app.touch.keyboard_requests, 0);
    touch(&mut app, 1, TouchPhase::Ended, x, y);
    assert_eq!(app.touch.keyboard_requests, 1);
    let selected = app.editor.active().unwrap().selection;
    assert!(selected.is_empty());
    assert_eq!(selected.head, 3);
    touch(&mut app, 2, TouchPhase::Started, x, y);
    touch(&mut app, 2, TouchPhase::Ended, x, y);
    assert_eq!(
        app.editor.active().unwrap().selection,
        winc_core::Selection::new(0, 5)
    );
    assert_eq!(app.drag, Drag::None);
    assert_eq!(app.touch.keyboard_requests, 2);
}

fn tap(app: &mut App, x: f32, y: f32) {
    touch(app, 1, TouchPhase::Started, x, y);
    touch(app, 1, TouchPhase::Ended, x, y);
}

#[test]
fn ime_and_touch_keyboard_share_editability_and_clear_stale_composition() {
    let mut app = document();
    assert!(app.takes_text());
    // 窗口暂时失焦不等于控件失去可编辑状态。
    app.window.focused = false;
    app.update_ime_area();
    assert!(app.takes_text());
    app.on_ime(winit::event::Ime::Preedit("组字".into(), None));
    app.focus = Focus::Tree;
    app.dirty = false;
    app.update_ime_area();
    assert!(!app.takes_text());
    assert!(app.preedit.is_empty());
    assert!(app.dirty);

    app.open_search(false);
    assert!(app.takes_text());
    app.on_ime(winit::event::Ime::Preedit("字段组字".into(), None));
    assert!(!app.field_preedit.is_empty());
    app.close_search();
    app.editor.active_mut().unwrap().readonly = true;
    app.update_ime_area();
    assert!(!app.takes_text());
    assert!(app.field_preedit.is_empty());

    app.editor.active_mut().unwrap().readonly = false;
    assert!(app.takes_text());
    let text = app.layout.text;
    app.on_mouse_move(text.x + 100.0, text.y + 40.0);
    app.on_mouse_down(MouseButton::Right);
    assert!(app.popup().is_some());
    assert!(!app.takes_text());
    app.dismiss_popup();
    assert!(app.takes_text());
}

#[test]
fn touch_keyboard_only_opens_for_editable_text_not_mouse_or_readonly_text() {
    let mut app = document();
    let x = app.layout.text.x + TEXT_PAD + app.cell * 3.0;
    let y = app.layout.text.y + app.fonts.line_height / 2.0;
    app.on_mouse_move(x, y);
    app.on_mouse_down(MouseButton::Left);
    app.on_mouse_up(MouseButton::Left);
    assert_eq!(app.touch.keyboard_requests, 0);
    tap(&mut app, x, y);
    assert_eq!(app.touch.keyboard_requests, 1);
    app.editor.active_mut().unwrap().readonly = true;
    tap(&mut app, x, y);
    assert_eq!(app.touch.keyboard_requests, 1);
    app.editor.active_mut().unwrap().readonly = false;
    let gutter = app.layout.gutter.x + 2.0;
    tap(&mut app, gutter, y);
    assert_eq!(app.touch.keyboard_requests, 1);
}

#[test]
fn touch_keyboard_distinguishes_editable_and_readonly_diff_sides() {
    use winc_editor::diff_view::{DiffSide, DiffView, LEFT, RIGHT};

    let mut app = document();
    let language = winc_syntax::detect("touch.rs");
    let view = DiffView::new(
        DiffSide::new("before", "let a = 1;\n", None, language),
        DiffSide::new(
            "after",
            "let a = 2;\n",
            Some(PathBuf::from("touch.rs")),
            language,
        ),
    );
    app.editor.show_diff("touch.rs", view);
    paint(&mut app);
    let view = app.editor.active().unwrap().diff.as_deref().unwrap();
    let parts = crate::app::diff_parts(app.layout.editor, view, app.cell);
    let y = parts.body.y + app.fonts.line_height / 2.0;
    let left = parts.panes[LEFT].text.x + 30.0;
    let right = parts.panes[RIGHT].text.x + 30.0;
    assert_eq!(app.touch_keyboard_at(left, y), None);
    assert_eq!(app.touch_keyboard_at(right, y), Some(Focus::Editor));
    // 历史差异的右侧同样只读，不能因为它与可编辑一侧使用相同布局而弹键盘。
    app.editor
        .active_mut()
        .unwrap()
        .diff
        .as_deref_mut()
        .unwrap()
        .new
        .path = None;
    tap(&mut app, right, y);
    assert_eq!(app.touch.keyboard_requests, 0);
}

#[test]
fn touch_keyboard_follows_search_and_settings_fields() {
    let mut app = document();
    app.open_search(true);
    paint(&mut app);
    for (focus, area) in [
        (Focus::Search, app.find.input.view.area),
        (Focus::Replace, app.find.replace_input.view.area),
    ] {
        let before = app.touch.keyboard_requests;
        tap(&mut app, area.x + 4.0, area.y + area.h / 2.0);
        assert_eq!(app.focus, focus);
        assert_eq!(app.touch.keyboard_requests, before + 1);
        app.on_ime(winit::event::Ime::Commit("触摸".into()));
        assert!(app.focused_field().unwrap().text().contains("触摸"));
    }
    app.open_settings();
    app.settings.motion = None;
    paint(&mut app);
    let area = app.settings.search.view.area;
    tap(&mut app, area.x + 4.0, area.y + area.h / 2.0);
    assert_eq!(app.focus, Focus::SettingsSearch);
    assert_eq!(app.touch.keyboard_requests, 3);
    // 设置浮层遮挡编辑区，点击遮罩关闭它时不能穿透弹键盘。
    let x = app.layout.text.x + 4.0;
    let y = app.layout.text.bottom() - 4.0;
    tap(&mut app, x, y);
    assert_eq!(app.touch.keyboard_requests, 3);
}

#[test]
fn touch_keyboard_does_not_open_when_dismissing_a_picker_or_context_menu() {
    let dir = sample_folder("touch-keyboard-picker");
    let mut app = app_with(&dir.join("short.rs"), false);
    paint(&mut app);
    let x = app.layout.text.x + TEXT_PAD + app.cell * 3.0;
    let y = app.layout.text.bottom() - 10.0;
    app.on_mouse_move(x, y);
    app.on_mouse_down(MouseButton::Right);
    assert!(app.popup().is_some());
    let outside = app.layout.text.y + 10.0;
    tap(&mut app, x + 40.0, outside);
    assert_eq!(app.touch.keyboard_requests, 0);
    app.open_picker();
    assert!(app.picker.open);
    app.picker.fold = None;
    paint(&mut app);
    let area = app.picker.input.view.area;
    tap(&mut app, area.x + 4.0, area.y + area.h / 2.0);
    assert_eq!(app.touch.keyboard_requests, 1);
    tap(&mut app, x, y);
    assert!(!app.picker.open);
    assert_eq!(app.touch.keyboard_requests, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn touch_keyboard_accepts_a_powershell_terminal_but_not_its_header() {
    let mut app = document();
    app.config.terminal_enabled = Some(true);
    app.terminal.open = true;
    app.terminal.height = crate::app::terminal::DEFAULT_HEIGHT;
    app.relayout();
    let size = app.terminal_size();
    let mut term = winc_pty::Terminal::new(usize::from(size.cols), usize::from(size.rows), 1000);
    term.feed(b"PS C:\\> ");
    let mut tab = crate::app::terminal::TermTab::new(1, size);
    tab.apply(term.take_frame().unwrap());
    tab.process = "pwsh.exe".into();
    app.terminal.tabs.push(tab);
    paint(&mut app);
    let grid = app.terminal_grid();
    let (x, y) = (
        grid.x + 4.5 * app.terminal_cell,
        grid.y + app.fonts.terminal_line_height / 2.0,
    );
    tap(&mut app, x, y);
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.touch.keyboard_requests, 1);
    let header = app.hits.terminal[0].0;
    assert_eq!(app.touch_keyboard_at(header.x + 2.0, header.y + 2.0), None);
    app.terminal.tab_mut().unwrap().ended = Some(crate::app::terminal::TabEnd::Exited(0));
    tap(&mut app, x, y);
    assert_eq!(app.touch.keyboard_requests, 1);
    app.terminal.tabs.clear();
    assert_eq!(app.touch_keyboard_at(x, y), None);
}

#[test]
fn double_tap_tolerates_finger_offset_but_mouse_keeps_tight_tolerance() {
    let mut app = document();
    let x = app.layout.text.x + TEXT_PAD + app.cell * 2.0;
    let y = app.layout.text.y + app.fonts.line_height / 2.0;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    touch(&mut app, 1, TouchPhase::Ended, x, y);
    // 第二次落指在两个方向上的偏移均超过鼠标的连击容差、小于 `TAP_SLOP`，且仍在同一个词内。
    let second = x + app.cell.min(10.0);
    touch(&mut app, 2, TouchPhase::Started, second, y + 6.0);
    touch(&mut app, 2, TouchPhase::Ended, second, y + 6.0);
    assert_eq!(
        app.editor.active().unwrap().selection,
        winc_core::Selection::new(0, 5)
    );
    // 鼠标点击仍按 4 逻辑像素的容差判断连击。
    app.last_click = None;
    assert_eq!(app.click_count(x, y), 1);
    assert_eq!(app.click_count(x + 6.0, y), 1);
}

#[test]
fn stale_touch_contact_stops_blocking_the_mouse() {
    let mut app = document();
    let text = app.layout.text;
    let (x, y) = (text.x + text.w / 2.0, text.y + text.h / 2.0);
    touch(&mut app, 1, TouchPhase::Started, x, y);
    assert!(
        app.touch_blocks_mouse(),
        "接触点未结束时应忽略由触摸提升的鼠标事件"
    );
    // 模拟抬手事件丢失：超过 `STALE` 没有新的触摸事件时接触点失效，鼠标恢复响应；
    // 随后到达的抬手事件不再定位光标。
    app.touch.last_event = Some(Instant::now() - Duration::from_secs(2));
    assert!(!app.touch_blocks_mouse());
    assert!(!app.touch.active());
    touch(&mut app, 1, TouchPhase::Ended, x, y);
    assert_eq!(
        app.editor.active().unwrap().selection,
        winc_core::Selection::point(0)
    );
}

#[test]
fn caption_controls_cover_top_bar_targets() {
    use crate::app::input::touch::WindowTouch;
    let verify = |app: &App| {
        let controls = app.caption_controls();
        let size = app.layout.window;
        for y in (0..TOP_BAR_END as usize).step_by(2) {
            for x in (0..size.w as usize).step_by(2) {
                let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
                // 系统命中测试先判断窗口边缘、后判断控件（`WindowHitTest::hit`），边缘处无须检查。
                let window = app.touch_window_at(x, y);
                if matches!(window, Some(WindowTouch::Resize(_))) {
                    continue;
                }
                let caption = window == Some(WindowTouch::Move);
                let control = controls.iter().any(|r| r.contains(x, y));
                assert!(caption || control, "顶栏控件未列入客户区：({x}, {y})");
                assert!(!(caption && control), "顶栏空白处被列入客户区：({x}, {y})");
            }
        }
    };
    let mut app = document();
    app.window.controls = true;
    app.relayout();
    paint(&mut app);
    verify(&app);
    app.sidebar.visible = true;
    app.relayout();
    paint(&mut app);
    verify(&app);
    app.new_file();
    app.new_file();
    paint(&mut app);
    verify(&app);
    app.config.terminal_enabled = Some(true);
    app.terminal.open = true;
    app.terminal
        .tabs
        .push(crate::app::terminal::TermTab::new(1, app.terminal_size()));
    app.config.terminal_right = Some(true);
    app.relayout();
    paint(&mut app);
    verify(&app);
}

#[test]
fn touch_scrolls_by_logical_pixels_without_moving_the_selection() {
    let mut app = document();
    let text = app.layout.text;
    let x = text.x + text.w / 2.0;
    let y = text.y + text.h / 2.0;
    let selection = app.editor.active().unwrap().selection;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    touch(&mut app, 1, TouchPhase::Moved, x, y - 100.0);
    touch(&mut app, 1, TouchPhase::Ended, x, y - 100.0);
    let doc = app.editor.active().unwrap();
    assert!((doc.scroll_line * f64::from(app.fonts.line_height) - 100.0).abs() < 0.01);
    assert_eq!(doc.selection, selection);
    assert_eq!(app.drag, Drag::None);
    assert!(app.wheel.glide.is_none());
    assert_eq!(app.touch.keyboard_requests, 0);
}

#[test]
fn touch_scroll_stays_in_the_panel_where_it_started() {
    let dir = sample_folder("touch-scroll-tree");
    for i in 0..80 {
        std::fs::write(dir.join(format!("file-{i:02}.txt")), "hello").unwrap();
    }
    let mut app = app_with(&dir, false);
    paint(&mut app);
    let tree = app.layout.tree;
    // 在文件树右侧（滚动条以左）落指，竖直滑动 100 逻辑像素的同时越过边界进入编辑区；竖直位移
    // 大于横向位移，按竖直方向滚动。
    let x = tree.right() - 40.0;
    let y = tree.y + tree.h / 2.0;
    assert!(app.touch_drag_point(x, y).is_none());
    let editor_x = app.layout.editor.x + 10.0;
    assert!(editor_x - x < 100.0);
    touch(&mut app, 1, TouchPhase::Started, x, y);
    touch(&mut app, 1, TouchPhase::Moved, editor_x, y - 100.0);
    touch(&mut app, 1, TouchPhase::Ended, x, y - 100.0);
    assert_eq!(app.tree.scroll, 100.0);
    assert!(app.editor.documents.is_empty());
    assert!(app.tree.drag.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn touch_long_press_on_text_defers_the_menu_until_release() {
    let mut app = document();
    let x = app.layout.text.x + 100.0;
    let y = app.layout.text.y + 100.0;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    let due = app.touch_tick(Instant::now()).unwrap();
    assert!(app.popup().is_none());
    assert_eq!(app.touch_tick(due), None);
    assert!(app.popup().is_none());
    assert!(matches!(app.drag, Drag::Select { .. }));
    touch(&mut app, 1, TouchPhase::Ended, x, y);
    assert!(app.popup().is_some());
    assert_eq!(app.drag, Drag::None);
    assert_eq!(app.touch.keyboard_requests, 0);
}

fn hold(app: &mut App, x: f32, y: f32) {
    touch(app, 1, TouchPhase::Started, x, y);
    let due = app.touch_tick(Instant::now()).unwrap();
    assert_eq!(app.touch_tick(due), None);
}

#[test]
fn touch_layout_dividers_accept_finger_offsets_and_keep_dragging() {
    let dir = sample_folder("touch-layout-dividers");
    let path = dir.join("layout.md");
    std::fs::write(&path, "# Layout\n\nhello world\n").unwrap();
    for kind in 0..5 {
        for offset in [0.0, -8.0, 8.0] {
            let mut app = if kind == 4 {
                // 没有项目的文档仍可展开大纲，且没有文件树的水平滚动条覆盖命中带。
                let mut app = blank_app(false);
                app.new_file();
                app
            } else {
                app_with(&path, false)
            };
            if kind == 4 {
                app.run_action(crate::keymap::Action::ShowOutline);
            } else if kind == 3 {
                app.set_preview_mode(winc_editor::PreviewMode::Split);
            } else if kind != 0 {
                app.config.terminal_enabled = Some(true);
                app.terminal.open = true;
                app.terminal.height = crate::app::terminal::DEFAULT_HEIGHT;
                app.config.terminal_right = Some(kind == 2);
                app.relayout();
            }
            paint(&mut app);
            let (area, before) = match kind {
                0 => (app.layout.splitter, app.sidebar.width),
                1 => (app.layout.terminal_divider, app.layout.terminal.h),
                2 => (app.layout.terminal_divider, app.layout.terminal.w),
                3 => (app.layout.split_divider, app.layout.editor.w),
                _ => (app.outline_divider(), app.outline_list().h),
            };
            assert!(area.w > 0.0 && area.h > 0.0);
            let (x, y) = (
                area.x + area.w / 2.0 + if matches!(kind, 1 | 4) { 0.0 } else { offset },
                area.y + area.h / 2.0 + if matches!(kind, 1 | 4) { offset } else { 0.0 },
            );
            if kind == 4 && offset > 0.0 {
                // 大纲标题行本身是展开/收起按钮，扩大命中带不能抢走它的点按。
                assert_eq!(app.touch_drag_point(x, y), None);
                tap(&mut app, x, y);
                assert!(!app.outline.open);
                assert_eq!(app.drag, Drag::None);
                continue;
            }
            touch(&mut app, 1, TouchPhase::Started, x, y);
            assert!(
                matches!(
                    app.drag,
                    Drag::Splitter { .. }
                        | Drag::TerminalDivider { .. }
                        | Drag::SplitDivider { .. }
                        | Drag::OutlineDivider { .. }
                ),
                "kind={kind}, offset={offset}, drag={:?}",
                app.drag
            );
            let (dx, dy) = match kind {
                1 | 4 => (0.0, -40.0),
                2 => (-40.0, 0.0),
                _ => (40.0, 0.0),
            };
            for step in 1..=4 {
                touch(
                    &mut app,
                    1,
                    TouchPhase::Moved,
                    x + dx * step as f32 / 4.0,
                    y + dy * step as f32 / 4.0,
                );
                paint(&mut app);
            }
            touch(&mut app, 1, TouchPhase::Ended, x + dx, y + dy);
            let after = match kind {
                0 => app.sidebar.width,
                1 => app.layout.terminal.h,
                2 => app.layout.terminal.w,
                3 => app.layout.editor.w,
                _ => app.outline_list().h,
            };
            assert!(
                (after - before - 40.0).abs() <= 1.0,
                "kind={kind}, offset={offset}, before={before}, after={after}"
            );
            assert_eq!(app.drag, Drag::None);
            assert!(app.popup().is_none());
            assert_eq!(app.touch.keyboard_requests, 0);
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn touch_log_divider_uses_the_rendered_hit_band_and_keeps_dragging() {
    let mut app = blank_app(false);
    app.new_file();
    let mut log = winc_editor::git::log::LogView::default();
    let request = log.begin().unwrap();
    log.finish(
        request.generation,
        Ok(vec![winc_editor::git::Commit {
            oid: "a".repeat(40),
            parents: Vec::new(),
            author: "Author".into(),
            email: "author@example.com".into(),
            time: 0,
            local: "1970-01-01 00:00".into(),
            refs: Vec::new(),
            subject: "Touch layout".into(),
        }]),
    );
    app.editor.active_mut().unwrap().log = Some(Box::new(log));
    app.relayout();
    paint(&mut app);
    let divider = app
        .hits
        .log
        .iter()
        .find(|(_, target)| *target == crate::app::git::log::LogTarget::Divider)
        .unwrap()
        .0;
    let (x, y) = (divider.x + divider.w / 2.0, divider.y + divider.h / 2.0);
    for offset in [-12.0, -8.0, 8.0, 12.0] {
        assert_eq!(app.touch_drag_point(x + offset, y), Some((x, y)));
    }
    let before = app.log_parts().details.w;
    touch(&mut app, 1, TouchPhase::Started, x + 8.0, y);
    assert!(matches!(app.drag, Drag::LogDivider { .. }));
    assert_eq!(app.log_parts().details.w, before);
    touch(&mut app, 1, TouchPhase::Moved, x - 32.0, y);
    paint(&mut app);
    touch(&mut app, 1, TouchPhase::Ended, x - 32.0, y);
    assert!((app.log_parts().details.w - before - 40.0).abs() < 1.0);
    assert_eq!(app.drag, Drag::None);
    assert!(app.popup().is_none());
    assert_eq!(app.touch.keyboard_requests, 0);
}

#[test]
fn touch_divider_hit_bands_keep_their_boundary_and_respect_overlays() {
    let dir = sample_folder("touch-divider-hit-bands");
    let mut app = app_with(&dir.join("short.rs"), false);
    app.set_sidebar(true);
    paint(&mut app);
    let divider = app.layout.splitter;
    let center = divider.x + divider.w / 2.0;
    let y = divider.y + divider.h / 2.0;
    for offset in [-12.25, -12.0, 12.0, 12.25] {
        assert_eq!(
            app.touch_drag_point(center + offset, y),
            (offset.abs() <= 12.0).then_some((center, y)),
            "offset={offset}"
        );
    }
    // 顶栏中的控件不能因扩大分隔条的命中带而变成布局拖动。
    let button = app.layout.sidebar_button;
    assert_eq!(
        app.touch_drag_point(button.x + button.w / 2.0, button.y + button.h / 2.0),
        None
    );
    app.on_mouse_move(center + 8.0, y);
    app.on_mouse_down(MouseButton::Right);
    assert!(app.popup().is_some());
    assert_eq!(app.touch_drag_point(center + 8.0, y), None);
    app.dismiss_popup();
    app.open_picker();
    assert!(app.picker.open);
    assert_eq!(app.touch_drag_point(center + 8.0, y), None);
    app.close_picker();
    app.open_settings();
    app.settings.motion = None;
    paint(&mut app);
    assert_eq!(app.touch_drag_point(center + 8.0, y), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn touch_long_press_drags_and_copies_multiline_text_in_both_directions() {
    for (readonly, reverse) in [(false, false), (false, true), (true, false)] {
        let mut app = document();
        app.editor.active_mut().unwrap().readonly = readonly;
        let at = |app: &App, col: f32, row: f32| {
            (
                app.layout.text.x + TEXT_PAD + app.cell * col,
                app.layout.text.y + app.fonts.line_height * (row + 0.5),
            )
        };
        let a = at(&app, 3.0, 0.0);
        let b = at(&app, 5.0, 1.0);
        let (start, end) = if reverse { (b, a) } else { (a, b) };
        hold(&mut app, start.0, start.1);
        assert!(app.popup().is_none());
        touch(&mut app, 1, TouchPhase::Moved, end.0, end.1);
        assert!(app.popup().is_none());
        let selection = app.editor.active().unwrap().selection;
        assert_eq!(selection.range(), 3..17);
        assert_eq!(app.editor.active().unwrap().scroll_line, 0.0);
        touch(&mut app, 1, TouchPhase::Ended, end.0, end.1);
        assert_eq!(app.editor.active().unwrap().selection, selection);
        assert!(menu_commands(&app).contains(&(MenuCommand::Copy, true)));
        assert_eq!(app.drag, Drag::None);
        assert_eq!(app.touch.keyboard_requests, 0);
        app.run_menu_command(MenuCommand::Copy);
        assert_eq!(app.clipboard_text().as_deref(), Some("lo world\nhello"));
    }
}

#[test]
fn touch_selection_is_cancelled_without_a_menu_or_stuck_drag() {
    for cancel in 0..3 {
        let mut app = document();
        let x = app.layout.text.x + TEXT_PAD + app.cell;
        let y = app.layout.text.y + app.fonts.line_height / 2.0;
        hold(&mut app, x, y);
        let end_x = x + app.cell * 3.0;
        touch(&mut app, 1, TouchPhase::Moved, end_x, y);
        match cancel {
            0 => touch(&mut app, 1, TouchPhase::Cancelled, x, y),
            1 => app.cancel_touch(),
            _ => touch(&mut app, 2, TouchPhase::Started, x, y),
        }
        let selection = app.editor.active().unwrap().selection;
        touch(&mut app, 1, TouchPhase::Ended, x, y);
        touch(&mut app, 2, TouchPhase::Ended, x, y);
        app.on_mouse_move(x + 100.0, y + 100.0);
        assert_eq!(app.editor.active().unwrap().selection, selection);
        assert_eq!(app.drag, Drag::None);
        assert!(app.popup().is_none());
        assert!(!app.touch.active());
    }
}

#[test]
fn touch_selection_menu_stays_with_the_field_when_released_outside_it() {
    let mut app = document();
    app.open_search(true);
    app.find.input.insert("hello world");
    paint(&mut app);
    let area = app.find.input.view.area;
    hold(&mut app, area.x, area.y + area.h / 2.0);
    assert_eq!(app.drag, Drag::Field);
    let end = (area.right() + 30.0, area.y + area.h / 2.0);
    touch(&mut app, 1, TouchPhase::Moved, end.0, end.1);
    touch(&mut app, 1, TouchPhase::Ended, end.0, end.1);
    assert_eq!(
        app.context_menu.as_ref().unwrap().target,
        MenuTarget::Field(Focus::Search)
    );
    // 通过触摸点按菜单项完成复制，覆盖抬手后下一次接触的完整路径。
    paint(&mut app);
    let menu = app.context_menu.as_ref().unwrap();
    let level = &app.popup().unwrap().levels[0];
    let copy = level
        .rows
        .iter()
        .position(|row| {
            matches!(row,
                PopupRow::Item { tag, .. }
                if MenuEntry::command_at(&menu.entries, *tag) == Some(MenuCommand::Copy)
            )
        })
        .unwrap();
    let rect = level.row_rects[copy];
    tap(&mut app, rect.x + 10.0, rect.y + rect.h / 2.0);
    assert!(app.popup().is_none());
    assert_eq!(app.clipboard_text().as_deref(), Some("hello world"));
    assert_eq!(app.focus, Focus::Search);
    assert_eq!(app.touch.keyboard_requests, 0);
}

#[test]
fn touch_field_menu_edits_the_field_without_changing_the_document() {
    let mut app = document();
    app.open_search(true);
    app.find.input.insert("hello world");
    paint(&mut app);
    let before = app.editor.active().unwrap().selection;
    let area = app.find.input.view.area;
    hold(&mut app, area.x + 4.0, area.y + area.h / 2.0);
    touch(
        &mut app,
        1,
        TouchPhase::Ended,
        area.x + 4.0,
        area.y + area.h / 2.0,
    );
    app.run_menu_command(MenuCommand::SelectAll);
    assert_eq!(app.find.input.selected_text(), "hello world");
    assert_eq!(app.editor.active().unwrap().selection, before);
    app.open_context_menu(MenuTarget::Field(Focus::Search), (area.x, area.y));
    app.run_menu_command(MenuCommand::Cut);
    assert_eq!(app.clipboard_text().as_deref(), Some("hello world"));
    assert_eq!(app.find.input.text(), "");
    assert_eq!(app.search.query, "");
    app.open_context_menu(MenuTarget::Field(Focus::Search), (area.x, area.y));
    app.run_menu_command(MenuCommand::Paste);
    assert_eq!(app.find.input.text(), "hello world");
    assert_eq!(app.search.query, "hello world");
    assert_eq!(
        app.editor
            .active()
            .unwrap()
            .text()
            .rope()
            .unwrap()
            .to_string(),
        "hello world\n".repeat(400)
    );
    assert_eq!(app.focus, Focus::Search);
}

#[test]
fn touch_long_press_copies_terminal_text_even_with_mouse_reporting() {
    for reporting in [false, true] {
        let mut app = document();
        app.config.terminal_enabled = Some(true);
        app.terminal.open = true;
        app.terminal.height = crate::app::terminal::DEFAULT_HEIGHT;
        app.relayout();
        let size = app.terminal_size();
        let mut term = winc_pty::Terminal::new(size.cols.into(), size.rows.into(), 1000);
        term.feed(b"PS C:\\> hello world");
        if reporting {
            term.feed(b"\x1b[?1000h");
        }
        let mut tab = crate::app::terminal::TermTab::new(1, size);
        tab.apply(term.take_frame().unwrap());
        app.terminal.tabs.push(tab);
        paint(&mut app);
        let grid = app.terminal_grid();
        let x = grid.x + 8.0 * app.terminal_cell;
        let end_x = grid.x + 19.0 * app.terminal_cell;
        let y = grid.y + app.fonts.terminal_line_height / 2.0;
        hold(&mut app, x, y);
        assert_eq!(app.drag, Drag::TerminalSelect);
        assert!(app.popup().is_none());
        touch(&mut app, 1, TouchPhase::Moved, end_x, y);
        touch(&mut app, 1, TouchPhase::Ended, end_x, y);
        assert_eq!(app.drag, Drag::None);
        assert!(menu_commands(&app).contains(&(MenuCommand::Copy, true)));
        app.run_menu_command(MenuCommand::Copy);
        assert_eq!(app.clipboard_text().as_deref(), Some("hello world"));
        assert_eq!(app.touch.keyboard_requests, 0);
    }
}

#[test]
fn touch_long_press_copies_readonly_diff_text() {
    use winc_editor::diff_view::{DiffSide, DiffView, LEFT};

    let mut app = document();
    let language = winc_syntax::detect("touch.rs");
    app.editor.show_diff(
        "touch.rs",
        DiffView::new(
            DiffSide::new("before", "let a = 1;\n", None, language),
            DiffSide::new("after", "let a = 2;\n", None, language),
        ),
    );
    paint(&mut app);
    let view = app.editor.active().unwrap().diff.as_deref().unwrap();
    let parts = crate::app::diff_parts(app.layout.editor, view, app.cell);
    let x = parts.panes[LEFT].text.x + TEXT_PAD;
    let end_x = x + 9.0 * app.cell;
    let y = parts.body.y + app.fonts.line_height / 2.0;
    hold(&mut app, x, y);
    assert!(app.popup().is_none());
    touch(&mut app, 1, TouchPhase::Moved, end_x, y);
    touch(&mut app, 1, TouchPhase::Ended, end_x, y);
    assert!(matches!(
        app.context_menu.as_ref().unwrap().target,
        MenuTarget::Diff(_, false)
    ));
    app.run_menu_command(MenuCommand::Copy);
    assert_eq!(app.clipboard_text().as_deref(), Some("let a = 1"));
    assert_eq!(app.touch.keyboard_requests, 0);
}

#[test]
fn touch_long_press_selects_markdown_links_without_navigation() {
    let dir = sample_folder("touch-markdown-selection");
    let path = dir.join("README.md");
    std::fs::write(&path, "[hello world](other.md)\n").unwrap();
    std::fs::write(dir.join("other.md"), "other\n").unwrap();
    let mut app = app_with(&path, false);
    app.set_preview_mode(winc_editor::PreviewMode::Split);
    for _ in 0..4 {
        app.wait_for_markdown();
        paint(&mut app);
    }
    let link = app.hits.markdown_links[0].0;
    let y = link.y + link.h / 2.0;
    hold(&mut app, link.x + 0.1, y);
    assert!(app.popup().is_none());
    touch(&mut app, 1, TouchPhase::Moved, link.right(), y);
    touch(&mut app, 1, TouchPhase::Ended, link.right(), y);
    assert_eq!(
        app.context_menu.as_ref().unwrap().target,
        MenuTarget::Markdown
    );
    app.run_menu_command(MenuCommand::Copy);
    assert_eq!(app.clipboard_text().as_deref(), Some("hello world"));
    assert_eq!(app.editor.active().unwrap().path.as_ref(), Some(&path));
    assert_eq!(app.touch.keyboard_requests, 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn touch_long_press_on_a_tab_keeps_the_existing_menu_behavior() {
    let mut app = document();
    let tab = app.hits.tabs[0].tab;
    hold(&mut app, tab.x + 30.0, tab.y + tab.h / 2.0);
    assert!(matches!(
        app.context_menu.as_ref().unwrap().target,
        MenuTarget::Tab(_)
    ));
    touch(
        &mut app,
        1,
        TouchPhase::Ended,
        tab.x + 30.0,
        tab.y + tab.h / 2.0,
    );
    assert!(app.popup().is_some());
    assert_eq!(app.drag, Drag::None);
}

#[test]
fn cancelled_or_interrupted_touch_never_places_the_caret() {
    let mut app = document();
    let x = app.layout.text.x + 100.0;
    let y = app.layout.text.y + 100.0;
    for cancel in [true, false] {
        touch(&mut app, 1, TouchPhase::Started, x, y);
        if cancel {
            touch(&mut app, 1, TouchPhase::Cancelled, x, y);
        } else {
            app.cancel_touch();
        }
        touch(&mut app, 1, TouchPhase::Ended, x, y);
        assert_eq!(app.editor.active().unwrap().selection.head, 0);
        assert_eq!(app.drag, Drag::None);
        assert!(!app.touch.active());
        assert_eq!(app.touch.keyboard_requests, 0);
    }
}

#[test]
fn touch_drags_the_scrollbar_and_cancellation_releases_it() {
    let mut app = document();
    let editor = app.layout.editor;
    let x = editor.right() - 4.0;
    let y = editor.y + editor.h / 2.0;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    assert!(matches!(app.drag, Drag::Scrollbar { .. }));
    let before = app.editor.active().unwrap().scroll_line;
    touch(&mut app, 1, TouchPhase::Moved, x, y + 50.0);
    assert!(app.editor.active().unwrap().scroll_line > before);
    touch(&mut app, 1, TouchPhase::Cancelled, x, y + 50.0);
    assert_eq!(app.drag, Drag::None);
    let after = app.editor.active().unwrap().scroll_line;
    app.on_mouse_move(x, y + 100.0);
    assert_eq!(app.editor.active().unwrap().scroll_line, after);
    touch(&mut app, 2, TouchPhase::Started, x, y);
    assert!(matches!(app.drag, Drag::Scrollbar { .. }));
    app.cancel_touch();
    assert_eq!(app.drag, Drag::None);
    assert_eq!(app.hover, Hover::None);
    assert_eq!(app.touch.keyboard_requests, 0);
}

#[test]
fn touch_scrolls_settings_without_scrolling_the_document_below() {
    let mut app = document();
    app.open_settings();
    app.settings.motion = None;
    app.settings_show_page(SettingsPage::Keys);
    paint(&mut app);
    let body = app.settings_layout().body;
    let x = body.x + body.w / 2.0;
    let y = body.y + body.h / 2.0;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    touch(&mut app, 1, TouchPhase::Moved, x, y - 60.0);
    touch(&mut app, 1, TouchPhase::Ended, x, y - 60.0);
    assert_eq!(app.settings.scroll, 60.0);
    assert_eq!(app.editor.active().unwrap().scroll_line, 0.0);
    assert_eq!(app.drag, Drag::None);
}

#[test]
fn touch_closes_a_tab_only_on_tap_not_swipe_or_cancel() {
    let mut app = blank_app(false);
    app.new_file();
    paint(&mut app);
    let close = app.hits.tabs[0].close;
    let x = close.x + close.w / 2.0;
    let y = close.y + close.h / 2.0;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    assert_eq!(app.editor.documents.len(), 1);
    touch(&mut app, 1, TouchPhase::Cancelled, x, y);
    assert_eq!(app.editor.documents.len(), 1);
    touch(&mut app, 2, TouchPhase::Started, x, y);
    touch(&mut app, 2, TouchPhase::Moved, x - 30.0, y);
    touch(&mut app, 2, TouchPhase::Ended, x, y);
    assert_eq!(app.editor.documents.len(), 1);
    touch(&mut app, 3, TouchPhase::Started, x, y);
    touch(&mut app, 3, TouchPhase::Ended, x, y);
    assert!(app.editor.documents.is_empty());
}

#[test]
fn additional_fingers_end_a_scrollbar_drag_without_starting_a_new_action() {
    let mut app = document();
    let editor = app.layout.editor;
    let x = editor.right() - 4.0;
    let y = editor.y + editor.h / 2.0;
    touch(&mut app, 1, TouchPhase::Started, x, y);
    assert!(matches!(app.drag, Drag::Scrollbar { .. }));
    touch(&mut app, 2, TouchPhase::Started, x - 100.0, y);
    assert_eq!(app.drag, Drag::None);
    let scroll = app.editor.active().unwrap().scroll_line;
    touch(&mut app, 1, TouchPhase::Ended, x, y);
    touch(&mut app, 2, TouchPhase::Moved, x - 100.0, y - 100.0);
    touch(&mut app, 2, TouchPhase::Ended, x - 100.0, y - 100.0);
    assert_eq!(app.editor.active().unwrap().scroll_line, scroll);
    assert!(app.popup().is_none());
    assert!(!app.touch.active());
    assert_eq!(app.touch.keyboard_requests, 0);
}

#[test]
fn touch_title_bar_starts_on_contact_without_scrolling_or_long_press() {
    let mut app = document();
    app.window.controls = true;
    app.relayout();
    paint(&mut app);
    let x = app.layout.window.w / 2.0;
    let y = ISLAND_MARGIN + TOP_BAR / 2.0;
    assert_eq!(
        app.touch_window_at(x, y),
        Some(crate::app::input::touch::WindowTouch::Move)
    );
    touch(&mut app, 1, TouchPhase::Started, x, y);
    assert!(
        app.title_click.is_some(),
        "顶栏按下应在落指时记录，而非等到抬手"
    );
    assert!(
        app.touch.active(),
        "抬手前应保留接触点，以忽略由触摸提升的鼠标事件"
    );
    assert_eq!(
        app.touch_tick(Instant::now() + Duration::from_secs(1)),
        None
    );
    touch(&mut app, 1, TouchPhase::Moved, x, y - 100.0);
    touch(&mut app, 1, TouchPhase::Ended, x, y);
    assert!(!app.touch.active());
    assert!(app.title_click.is_some(), "抬手不应再记录一次顶栏按下");
    assert_eq!(app.editor.active().unwrap().scroll_line, 0.0);
    assert_eq!(app.drag, Drag::None);
    // 第二次落指构成双击，并清除前一次按下的记录。
    touch(&mut app, 2, TouchPhase::Started, x, y);
    assert!(app.title_click.is_none());
}

#[test]
fn touch_window_edges_are_distinct_from_buttons_and_overlays() {
    use crate::app::input::touch::WindowTouch;
    use winit::window::ResizeDirection;
    let mut app = document();
    app.window.controls = true;
    app.relayout();
    paint(&mut app);
    assert_eq!(
        app.touch_window_at(1.0, 1.0),
        Some(WindowTouch::Resize(ResizeDirection::NorthWest))
    );
    touch(&mut app, 1, TouchPhase::Started, 1.0, 1.0);
    assert!(app.touch.active());
    touch(&mut app, 1, TouchPhase::Cancelled, 1.0, 1.0);
    let (close, _) = app.window_control_rects()[2];
    assert_eq!(
        app.touch_window_at(close.x + close.w / 2.0, close.y + close.h / 2.0),
        None
    );
    app.open_settings();
    assert_eq!(app.touch_window_at(app.layout.window.w / 2.0, 20.0), None);
    assert_eq!(app.touch_window_at(1.0, 1.0), None);
}

#[cfg(windows)]
#[test]
fn native_touch_caption_matches_application_targets() {
    use crate::app::input::touch::WindowTouch;
    use windows_sys::Win32::UI::WindowsAndMessaging::{HTCAPTION, HTCLIENT};
    let verify = |app: &App| {
        let native = app.window_hit_test();
        let size = app.layout.window;
        for y in (0..TOP_BAR_END as usize).step_by(2) {
            for x in (0..size.w as usize).step_by(2) {
                let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
                let hit = native.hit(x, y, size.w, size.h, false);
                let target = app.touch_window_at(x, y);
                assert_eq!(
                    hit == HTCAPTION,
                    target == Some(WindowTouch::Move),
                    "标题栏判定不一致：({x}, {y})"
                );
                assert_eq!(
                    hit == HTCLIENT,
                    target.is_none(),
                    "客户区判定不一致：({x}, {y})"
                );
            }
        }
    };
    let mut app = document();
    app.window.controls = true;
    app.relayout();
    paint(&mut app);
    verify(&app);
    app.sidebar.visible = true;
    app.relayout();
    paint(&mut app);
    verify(&app);
    app.new_file();
    app.new_file();
    paint(&mut app);
    verify(&app);
    app.config.terminal_enabled = Some(true);
    app.terminal.open = true;
    app.terminal
        .tabs
        .push(crate::app::terminal::TermTab::new(1, app.terminal_size()));
    app.config.terminal_right = Some(true);
    app.relayout();
    paint(&mut app);
    verify(&app);
    app.open_settings();
    verify(&app);
}
