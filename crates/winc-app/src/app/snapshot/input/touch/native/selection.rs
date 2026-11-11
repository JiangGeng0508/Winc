//! 用普通窗口接收 Windows 原生长按拖选，验证菜单直到抬手才出现。

use super::*;

#[test]
#[ignore = "opens Winc and injects native touch; requires an interactive Windows desktop"]
fn native_touch_drag_selection_in_rendered_app() {
    struct TestApp {
        app: App,
        proxy: winit::event_loop::EventLoopProxy<Result<(), String>>,
        commands: Option<std::sync::mpsc::Sender<(POINT, POINT)>>,
        start: Instant,
        ready_at: Instant,
        phase: usize,
        pending: bool,
        released: usize,
        selected: bool,
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
                while let Ok((start, end)) = receiver.recv() {
                    let result = crate::windows::inject_hold_drag(raw, start, end);
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
                    assert!(self.selected, "long press did not start selection");
                    self.released += 1;
                }
            }
            self.app.window_event(event_loop, id, event);
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.app.tick();
            if self.app.touch.active() {
                assert!(self.app.popup().is_none(), "menu opened during contact");
                self.selected |= matches!(self.app.drag, Drag::Select { .. } | Drag::Field);
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
                } else {
                    let field = app.find.input.view.area;
                    (
                        (field.x + 2.0, field.y + field.h / 2.0),
                        (field.right() - 2.0, field.y + field.h / 2.0),
                    )
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
                    .send((physical(start), physical(end)))
                    .unwrap();
                self.pending = false;
            }
            assert!(
                self.start.elapsed() < Duration::from_secs(15),
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
            assert!(menu_commands(&self.app).contains(&(MenuCommand::Copy, true)));
            self.app.run_menu_command(MenuCommand::Copy);
            let expected = if self.phase == 0 {
                "lo world\nhello"
            } else {
                "hello world"
            };
            assert_eq!(self.app.clipboard_text().as_deref(), Some(expected));
            assert_eq!(self.app.touch.keyboard_requests, 0);
            if self.phase == 1 {
                event_loop.exit();
                return;
            }
            self.phase += 1;
            self.app.open_search(true);
            self.app.find.input.select_all();
            self.app.find.input.insert("hello world");
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
    };
    event_loop.run_app(&mut test).unwrap();
    assert_eq!(test.released, 2);
}
