//! 自绘输入区通过 Windows 的桌面 `InputPane` 接口请求触摸键盘。
//!
//! 仅在确认输入区点按后调用，不轮询焦点、不启动外部程序，也不切换已显示的键盘。
//! 系统决定是否接受请求（例如前台窗口、硬件键盘与系统输入策略）。
//!
//! 桌面互操作接口（Windows 10 1607 起）：
//! <https://learn.microsoft.com/windows/win32/api/inputpaneinterop/nf-inputpaneinterop-iinputpaneinterop-getforwindow>

#![allow(unsafe_code)] // WinRT 初始化与当前窗口的 IInputPaneInterop。

use windows::UI::ViewManagement::InputPane;
use windows::Win32::Foundation::{E_HANDLE, HWND, RECT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::WinRT::{
    IInputPaneInterop, RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize,
};
use windows::Win32::UI::Shell::{FrameworkInputPane, IFrameworkInputPane};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

mod focus;
pub use focus::TouchKeyboard;

/// 为 `hwnd` 请求触摸键盘；已显示时保持原状。复用本线程的可见性查询接口。
///
/// # Errors
///
/// 查询可见性或调用 `InputPane` 失败时返回系统错误。
fn show_touch_keyboard(hwnd: HWND, input: &IFrameworkInputPane) -> windows::core::Result<bool> {
    // Location 查询屏幕上的键盘区域，不依赖它是否遮挡本窗口，浮动键盘也可识别。
    // SAFETY: input 是本线程上的有效 COM 接口。
    let location = unsafe { input.Location() }?;
    show_if_hidden(location, || {
        let interop = windows::core::factory::<InputPane, IInputPaneInterop>()?;
        // SAFETY: 调用方持有当前线程上的顶层窗口和 COM 初始化 guard；接口不逃逸到其他线程。
        let pane = unsafe { interop.GetForWindow::<InputPane>(hwnd) }?;
        pane.TryShow()
    })
}
/// 可见时不触发显示调用，包括在另一块屏幕上的键盘。
fn show_if_hidden(
    location: RECT,
    show: impl FnOnce() -> windows::core::Result<bool>,
) -> windows::core::Result<bool> {
    if location.right > location.left && location.bottom > location.top {
        Ok(true)
    } else {
        show()
    }
}

/// 在全部局部 COM 接口释放后，配对本次成功的 `RoInitialize`（包括 `S_FALSE`）。
struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: 只在成功初始化后构造，并在同一线程、本次调用退出时释放。
        unsafe { RoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::windows::EventLoopBuilderExtWindows;
    use winit::window::WindowId;

    #[test]
    fn visible_keyboard_skips_show_and_hidden_keyboard_requests_it() {
        let hidden = RECT::default();
        let docked = RECT {
            left: 0,
            top: 600,
            right: 1920,
            bottom: 1080,
        };
        let floating = RECT {
            left: -800,
            top: -400,
            right: -200,
            bottom: -100,
        };
        let mut requests = 0;
        for (location, expected) in [
            (hidden, 1),
            (docked, 1),
            (docked, 1),
            (floating, 1),
            (hidden, 2),
        ] {
            assert!(
                show_if_hidden(location, || {
                    requests += 1;
                    Ok(true)
                })
                .unwrap()
            );
            assert_eq!(requests, expected);
        }
    }

    #[test]
    #[ignore = "requires a Windows desktop; requests InputPane for a temporary window"]
    fn native_touch_keyboard_desktop_interop() {
        struct TestApp;
        impl ApplicationHandler for TestApp {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let window = event_loop
                    .create_window(Window::default_attributes().with_visible(false))
                    .unwrap();
                let window = std::sync::Arc::new(window);
                let mut keyboard = TouchKeyboard::default();
                // 验证真实的 WinRT 激活、HWND 互操作及接口复用；clear 后重新创建并配对释放。
                // TryShow 是 best effort；接受请求不代表键盘实际可见，结果由当前系统策略决定。

                for _ in 0..2 {
                    let _ = keyboard.show(&window).expect("InputPane desktop interop");
                    let _ = keyboard
                        .show(&window)
                        .expect("cached InputPane desktop interop");
                    keyboard.clear();
                }
                event_loop.exit();
            }

            fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        }
        let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
        event_loop.run_app(&mut TestApp).unwrap();
    }
}
