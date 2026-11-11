//! 普通 Winc 窗口的真实触摸回归；置顶窗口会掩盖系统自动收起，不能用于此测试。
#![allow(unsafe_code)]

use super::*;
use ::windows::UI::ViewManagement::InputPane;
use ::windows::Win32::Foundation::HWND;
use ::windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use ::windows::Win32::System::WinRT::{
    IInputPaneInterop, RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize,
};
use ::windows::Win32::UI::Accessibility::{
    AccessibleObjectFromWindow, IAccessible, ROLE_SYSTEM_TEXT,
};
use ::windows::Win32::UI::Shell::{FrameworkInputPane, IFrameworkInputPane};
use ::windows::core::Interface;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::Pointer::{InitializeTouchInjection, TOUCH_FEEDBACK_NONE};
use windows_sys::Win32::UI::WindowsAndMessaging::OBJID_CLIENT;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: 测试入口已成功在同一线程初始化 STA，所有 COM 对象先于本 guard 释放。
        unsafe { RoUninitialize() };
    }
}

fn hwnd(window: &Window) -> HWND {
    let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw() else {
        unreachable!()
    };
    HWND(handle.hwnd.get() as _)
}

fn native_role(window: &Window) -> i32 {
    // SAFETY: HWND 存活，输出类型按请求的 IID 转换；COM 已初始化。
    unsafe {
        let mut object = std::ptr::null_mut();
        AccessibleObjectFromWindow(
            hwnd(window),
            OBJID_CLIENT as u32,
            &IAccessible::IID,
            &mut object,
        )
        .unwrap();
        let object = IAccessible::from_raw(object);
        i32::try_from(&object.get_accRole(&0i32.into()).unwrap()).unwrap()
    }
}

#[test]
#[ignore = "opens Winc and injects native touch; requires an interactive Windows desktop"]
fn native_touch_keyboard_in_rendered_app() {
    // SAFETY: 事件循环在本测试线程运行，最后由 guard 反初始化。
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.unwrap();
    let _apartment = Apartment;
    // SAFETY: 已初始化 COM，创建系统可见性查询接口。
    let input: IFrameworkInputPane =
        unsafe { CoCreateInstance(&FrameworkInputPane, None, CLSCTX_INPROC_SERVER) }.unwrap();
    struct TestApp {
        app: App,
        proxy: winit::event_loop::EventLoopProxy<Result<(), String>>,
        input: IFrameworkInputPane,
        commands: Option<std::sync::mpsc::Sender<POINT>>,
        start: Instant,
        ready_at: Instant,
        phase: usize,
        pending: bool,
        waiting_for_hide: bool,
        shown: bool,
        hidden: bool,
        taps: usize,
    }
    impl TestApp {
        fn visible(&self) -> bool {
            // SAFETY: 查询当前线程上的有效 COM 接口。
            let r = unsafe { self.input.Location() }.unwrap();
            r.right > r.left && r.bottom > r.top
        }
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
                while let Ok(point) = receiver.recv() {
                    // 覆盖普通重复点击和快速双击/三击，结束后等待系统可能的异步收起。
                    let result = [1000, 100, 100, 1200].into_iter().try_for_each(|settle| {
                        crate::windows::inject_contact(raw, point, point, settle)
                    });
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
                eprintln!(
                    "phase={} {:?} touch {:?}, visible={}",
                    self.phase,
                    self.start.elapsed(),
                    touch.phase,
                    self.visible()
                );
                if touch.phase == TouchPhase::Ended {
                    self.taps += 1;
                }
            }
            self.app.window_event(event_loop, id, event);
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.app.tick();
            let visible = self.visible();
            if self.waiting_for_hide {
                if !visible {
                    self.waiting_for_hide = false;
                    self.shown = false;
                    self.ready_at = Instant::now() + Duration::from_millis(500);
                    self.app.window.handle.as_ref().unwrap().focus_window();
                }
            } else {
                self.hidden |= self.shown && !visible;
                self.shown |= visible;
            }
            if self.pending
                && !self.waiting_for_hide
                && Instant::now() >= self.ready_at
                && !self.app.dirty
                && self.app.window.focused
            {
                let window = self.app.window.handle.as_ref().unwrap();
                assert_eq!(
                    native_role(window),
                    ROLE_SYSTEM_TEXT as i32,
                    "phase {} focus {:?}",
                    self.phase,
                    self.app.focus
                );
                let (x, y) = match self.phase {
                    0 => (
                        self.app.layout.text.x + 100.0,
                        self.app.layout.text.y + 40.0,
                    ),
                    1 | 3 => {
                        let a = self.app.find.input.view.area;
                        (a.x + 8.0, a.y + a.h / 2.0)
                    }
                    2 => {
                        let a = self.app.find.replace_input.view.area;
                        (a.x + 8.0, a.y + a.h / 2.0)
                    }
                    _ => unreachable!(),
                };
                let scale = self.app.renderer.as_ref().unwrap().scale_factor();
                let origin = window.inner_position().unwrap();
                self.commands
                    .as_ref()
                    .unwrap()
                    .send(POINT {
                        x: origin.x + (x * scale) as i32,
                        y: origin.y + (y * scale) as i32,
                    })
                    .unwrap();
                self.pending = false;
            }
            assert!(
                self.start.elapsed() < Duration::from_secs(25),
                "native touch test timed out"
            );
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(20),
            ));
        }
        fn user_event(&mut self, event_loop: &ActiveEventLoop, result: Result<(), String>) {
            result.unwrap();
            assert!(
                self.shown && self.visible(),
                "phase {} never showed keyboard",
                self.phase
            );
            assert!(
                !self.hidden,
                "phase {} closed the visible keyboard",
                self.phase
            );
            assert_eq!(
                self.app.focus,
                match self.phase {
                    0 => Focus::Editor,
                    2 => Focus::Replace,
                    _ => Focus::Search,
                }
            );
            self.phase += 1;
            match self.phase {
                1 => self.app.open_search(true),
                2 => {}
                3 => {
                    // 模拟用户主动关闭键盘，确认之后仍可由点按重新打开。
                    let window = self.app.window.handle.as_ref().unwrap();
                    let interop =
                        ::windows::core::factory::<InputPane, IInputPaneInterop>().unwrap();
                    // SAFETY: 有效 HWND；接口仍在测试事件线程使用。
                    let pane = unsafe { interop.GetForWindow::<InputPane>(hwnd(window)) }.unwrap();
                    assert!(pane.TryHide().unwrap());
                    self.waiting_for_hide = true;
                }
                4 => {
                    event_loop.exit();
                    return;
                }
                _ => unreachable!(),
            }
            self.pending = true;
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
        input,
        commands: None,
        start: Instant::now(),
        ready_at: Instant::now() + Duration::from_secs(1),
        phase: 0,
        pending: true,
        waiting_for_hide: false,
        shown: false,
        hidden: false,
        taps: 0,
    };
    event_loop.run_app(&mut test).unwrap();
    assert_eq!(test.taps, 16);
    assert_eq!(test.app.touch.keyboard_requests, 16);
    // 只读焦点与窗口失焦都须撤销标注；恢复输入后可重新注册。
    test.app.focus = Focus::Editor;
    test.app.editor.active_mut().unwrap().readonly = true;
    test.app.update_ime_area();
    let window = test.app.window.handle.as_ref().unwrap();
    assert_ne!(native_role(window), ROLE_SYSTEM_TEXT as i32);
    test.app.editor.active_mut().unwrap().readonly = false;
    test.app.update_ime_area();
    let window = test.app.window.handle.as_ref().unwrap();
    assert_eq!(native_role(window), ROLE_SYSTEM_TEXT as i32);
    test.app.window.focused = false;
    test.app.update_ime_area();
    assert_ne!(
        native_role(test.app.window.handle.as_ref().unwrap()),
        ROLE_SYSTEM_TEXT as i32
    );
}
