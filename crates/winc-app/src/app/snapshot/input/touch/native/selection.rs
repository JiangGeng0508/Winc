//! 用普通窗口接收 Windows 原生拖选和布局拖动，验证菜单时机及分隔条的触摸命中。

use super::*;

#[test]
#[ignore = "opens Winc and injects native touch; requires an interactive Windows desktop"]
fn native_touch_drag_selection_in_rendered_app() {
    struct TestApp {
        app: App,
        proxy: winit::event_loop::EventLoopProxy<Result<(), String>>,
        commands: Option<std::sync::mpsc::Sender<(POINT, POINT, bool)>>,
        start: Instant,
        ready_at: Instant,
        phase: usize,
        pending: bool,
        released: usize,
        selected: bool,
        before: f32,
        markdown: PathBuf,
    }
    impl ApplicationHandler<Result<(), String>> for TestApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.app.resumed(event_loop);
            let window = self.app.window.handle.as_ref().unwrap();
            window.set_outer_position(PhysicalPosition::new(100, 100));
            window.focus_window();
            let raw = hwnd(window).0 as isize;
            let (sender, receiver) = std::sync::mpsc::channel();
            self.commands = Some(sender);
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                // SAFETY: 为此测试初始化单个接触点，没有指针参数。
                let initialized = unsafe { InitializeTouchInjection(1, TOUCH_FEEDBACK_NONE) };
                assert_ne!(initialized, 0);
                while let Ok((start, end, hold)) = receiver.recv() {
                    let result = if hold {
                        crate::windows::inject_hold_drag(raw, start, end)
                    } else {
                        crate::windows::inject_contact(raw, start, end, 300)
                    };
                    let failed = result.is_err();
                    let _ = proxy.send_event(result.map_err(|e| e.to_string()));
                    if failed {
                        break;
                    }
                }
            });
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            if let WindowEvent::Touch(touch) = &event {
                assert!(self.app.popup().is_none(), "menu opened before release");
                if touch.phase == TouchPhase::Ended {
                    assert!(self.selected, "phase {} did not start dragging", self.phase);
                    self.released += 1;
                }
            }
            self.app.window_event(event_loop, id, event);
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.app.tick();
            if self.app.touch.active() {
                assert!(self.app.popup().is_none(), "menu opened during contact");
                self.selected |= matches!(
                    self.app.drag,
                    Drag::Select { .. }
                        | Drag::Field
                        | Drag::Splitter { .. }
                        | Drag::TerminalDivider { .. }
                        | Drag::SplitDivider { .. }
                );
            }
            if self.pending
                && Instant::now() >= self.ready_at
                && !self.app.dirty
                && self.app.window.focused
            {
                let app = &self.app;
                let (start, end) = if self.phase == 0 {
                    let text = app.layout.text;
                    (
                        (
                            text.x + TEXT_PAD + app.cell * 3.0,
                            text.y + app.fonts.line_height * 0.5,
                        ),
                        (
                            text.x + TEXT_PAD + app.cell * 5.0,
                            text.y + app.fonts.line_height * 1.5,
                        ),
                    )
                } else if self.phase == 1 {
                    let field = app.find.input.view.area;
                    (
                        (field.x + 2.0, field.y + field.h / 2.0),
                        (field.right() - 2.0, field.y + field.h / 2.0),
                    )
                } else {
                    let (rect, size) = match self.phase {
                        2 => (app.layout.splitter, app.sidebar.width),
                        3 => (app.layout.terminal_divider, app.layout.terminal.h),
                        4 => (app.layout.terminal_divider, app.layout.terminal.w),
                        _ => (app.layout.split_divider, app.layout.editor.w),
                    };
                    self.before = size;
                    // 落指偏离细分隔条中心 8 逻辑像素，直接拖动，无需先长按。
                    let x = rect.x + rect.w / 2.0 + if self.phase == 3 { 0.0 } else { 8.0 };
                    let y = rect.y + rect.h / 2.0 + if self.phase == 3 { 8.0 } else { 0.0 };
                    let delta = match self.phase {
                        3 => (0.0, -40.0),
                        4 => (-40.0, 0.0),
                        _ => (40.0, 0.0),
                    };
                    ((x, y), (x + delta.0, y + delta.1))
                };
                let window = app.window.handle.as_ref().unwrap();
                let origin = window.inner_position().unwrap();
                let scale = app.renderer.as_ref().unwrap().scale_factor();
                let physical = |(x, y): (f32, f32)| POINT {
                    x: origin.x + (x * scale) as i32,
                    y: origin.y + (y * scale) as i32,
                };
                self.commands
                    .as_ref()
                    .unwrap()
                    .send((physical(start), physical(end), self.phase < 2))
                    .unwrap();
                self.pending = false;
            }
            assert!(
                self.start.elapsed() < Duration::from_secs(25),
                "native selection test timed out"
            );
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(20),
            ));
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, result: Result<(), String>) {
            result.unwrap();
            assert_eq!(self.released, self.phase + 1);
            assert_eq!(self.app.drag, Drag::None);
            if self.phase < 2 {
                assert!(menu_commands(&self.app).contains(&(MenuCommand::Copy, true)));
                self.app.run_menu_command(MenuCommand::Copy);
                let expected = if self.phase == 0 {
                    "lo world\nhello"
                } else {
                    "hello world"
                };
                assert_eq!(self.app.clipboard_text().as_deref(), Some(expected));
            } else {
                let after = match self.phase {
                    2 => self.app.sidebar.width,
                    3 => self.app.layout.terminal.h,
                    4 => self.app.layout.terminal.w,
                    _ => self.app.layout.editor.w,
                };
                assert!(
                    (after - self.before - 40.0).abs() <= 2.0,
                    "phase={}, before={}, after={after}",
                    self.phase,
                    self.before
                );
                assert!(self.app.popup().is_none());
            }
            assert_eq!(self.app.touch.keyboard_requests, 0);
            if self.phase == 5 {
                event_loop.exit();
                return;
            }
            self.phase += 1;
            match self.phase {
                1 => {
                    self.app.open_search(true);
                    self.app.find.input.select_all();
                    self.app.find.input.insert("hello world");
                }
                2 => {
                    self.app.close_search();
                    self.app.set_sidebar(true);
                }
                3 => {
                    self.app.config.terminal_enabled = Some(true);
                    self.app.terminal.open = true;
                    self.app.terminal.height = crate::app::terminal::DEFAULT_HEIGHT;
                    self.app.relayout();
                }
                4 => {
                    self.app.config.terminal_right = Some(true);
                    self.app.relayout();
                }
                _ => {
                    self.app.terminal.open = false;
                    let path = self.markdown.clone();
                    self.app.open_path(&path);
                    self.app.set_preview_mode(winc_editor::PreviewMode::Split);
                }
            }
            self.pending = true;
            self.selected = false;
            self.ready_at = Instant::now() + Duration::from_millis(500);
        }
    }
    let event_loop = EventLoop::<Result<(), String>>::with_user_event()
        .with_any_thread(true)
        .build()
        .unwrap();
    let mut app = App::new(
        Editor::default(),
        None,
        Bench::from_env(),
        Config::default(),
    );
    app.new_file();
    app.run(Command::Insert("hello world\n".repeat(20)));
    app.editor.active_mut().unwrap().selection = winc_core::Selection::point(0);
    let dir = sample_folder("native-touch-layout");
    let markdown = dir.join("layout.md");
    std::fs::write(&markdown, "# Layout\n\nhello world\n").unwrap();
    let mut test = TestApp {
        app,
        proxy: event_loop.create_proxy(),
        commands: None,
        start: Instant::now(),
        ready_at: Instant::now() + Duration::from_secs(1),
        phase: 0,
        pending: true,
        released: 0,
        selected: false,
        before: 0.0,
        markdown,
    };
    event_loop.run_app(&mut test).unwrap();
    assert_eq!(test.released, 6);
    let _ = std::fs::remove_dir_all(dir);
}
