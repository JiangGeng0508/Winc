//! 复用系统 HWND 的可访问性对象，通过动态标注暴露当前自绘输入区的文本角色。
//! 单独请求 `InputPane` 或创建系统 caret 都不能阻止普通窗口中下一次触摸引发的自动收起。
//! 文本角色、可聚焦状态和点按后的焦点通知必须配合使用。
//!
//! <https://learn.microsoft.com/windows/win32/api/oleacc/nf-oleacc-iaccpropservices-sethwndprop>

use super::*;
use std::sync::Arc;
use windows::Win32::UI::Accessibility::{
    CAccPropServices, IAccPropServices, NotifyWinEvent, PROPID_ACC_ROLE, PROPID_ACC_STATE,
    ROLE_SYSTEM_TEXT,
};
use windows_sys::Win32::UI::Controls::STATE_SYSTEM_FOCUSABLE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_FOCUS, EVENT_OBJECT_STATECHANGE, OBJID_CLIENT, STATE_SYSTEM_FOCUSED,
};

/// 当前窗口的文本输入标注，只在窗口线程使用；稳定焦点下不重复设置系统属性。
#[derive(Default)]
pub struct TouchKeyboard {
    input: Option<TextInput>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl TouchKeyboard {
    /// 同步窗口是否持有可编辑文本焦点，不依赖光标是否正在闪烁或位于可见范围内。
    ///
    /// # Errors
    ///
    /// 获取窗口句柄、初始化 COM 或标注属性失败时返回系统错误。
    pub fn update(&mut self, window: &Arc<Window>, editable: bool) -> windows::core::Result<()> {
        if !editable {
            self.clear();
            return Ok(());
        }
        if let Some(input) = self.input.as_mut()
            && Arc::ptr_eq(&input.window, window)
        {
            return input.sync_focus();
        }
        self.clear();
        let handle = window
            .window_handle()
            .map_err(|_| windows::core::Error::from_hresult(E_HANDLE))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(windows::core::Error::from_hresult(E_HANDLE));
        };
        let hwnd = HWND(handle.hwnd.get() as _);
        // SAFETY: 初始化成功后交给 TextInput，在同一窗口线程上最后释放。
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }?;
        let apartment = Apartment;
        // SAFETY: 已初始化 COM，不聚合；接口保留于当前窗口线程。
        let props = unsafe { CoCreateInstance(&CAccPropServices, None, CLSCTX_INPROC_SERVER) }?;
        let mut input = TextInput {
            hwnd,
            props,
            _apartment: apartment,
            window: window.clone(),
            focused: None,
        };
        // SAFETY: HWND 存活；只标注当前自绘输入焦点对应的客户区对象，不更改窗口对象及其他应用。
        unsafe {
            input.props.SetHwndProp(
                hwnd,
                OBJID_CLIENT as u32,
                CHILDID_SELF,
                PROPID_ACC_ROLE,
                &(ROLE_SYSTEM_TEXT as i32).into(),
            )?;
        }
        input.sync_focus()?;
        self.input = Some(input);
        Ok(())
    }

    /// 确认一次输入区点按后通知系统文本焦点，再按实际可见性请求键盘。
    ///
    /// # Errors
    ///
    /// 无法标注输入区或请求 `InputPane` 时返回系统错误。
    pub fn show(&mut self, window: &Arc<Window>) -> windows::core::Result<bool> {
        self.update(window, true)?;
        if let Some(input) = &self.input {
            // 即使焦点仍在同一自绘输入区，也须让系统重新查询其文本角色，取消本次触摸的自动收起。
            // SAFETY: input 持有窗口，当前点按已由应用确认属于可编辑文本。
            unsafe {
                NotifyWinEvent(
                    EVENT_OBJECT_FOCUS,
                    input.hwnd,
                    OBJID_CLIENT,
                    CHILDID_SELF as i32,
                )
            };
        }
        show_touch_keyboard(window)
    }

    /// 当前控件不再可编辑时撤销标注；窗口暂时失焦仍保留文本角色。
    pub fn clear(&mut self) {
        self.input = None;
    }
}

struct TextInput {
    hwnd: HWND,
    props: IAccPropServices,
    _apartment: Apartment,
    // 保证 ClearHwndProps 在 HWND 销毁之前调用；COM 接口先于 apartment 释放。
    window: Arc<Window>,
    focused: Option<bool>,
}

impl TextInput {
    /// 文本角色随控件保留，焦点状态随 HWND 同步；重新激活时立刻通知系统，无需等待下一次点按。
    fn sync_focus(&mut self) -> windows::core::Result<()> {
        // SAFETY: 查询当前窗口线程上的键盘焦点，窗口由 self.window 保持存活。
        let focused = unsafe { GetFocus() } == self.hwnd.0;
        if self.focused == Some(focused) {
            return Ok(());
        }
        let state = STATE_SYSTEM_FOCUSABLE | if focused { STATE_SYSTEM_FOCUSED } else { 0 };
        // SAFETY: COM 与 HWND 均在当前线程存活；只更新此输入对象的焦点状态。
        unsafe {
            self.props.SetHwndProp(
                self.hwnd,
                OBJID_CLIENT as u32,
                CHILDID_SELF,
                PROPID_ACC_STATE,
                &(state as i32).into(),
            )?;
            NotifyWinEvent(
                if focused {
                    EVENT_OBJECT_FOCUS
                } else {
                    EVENT_OBJECT_STATECHANGE
                },
                self.hwnd,
                OBJID_CLIENT,
                CHILDID_SELF as i32,
            );
        }
        self.focused = Some(focused);
        Ok(())
    }
}

impl Drop for TextInput {
    fn drop(&mut self) {
        // SAFETY: window 保持 HWND 存活，COM 尚未反初始化；只撤销本对象设置的两个属性。
        unsafe {
            let _ = self.props.ClearHwndProps(
                self.hwnd,
                OBJID_CLIENT as u32,
                CHILDID_SELF,
                &[PROPID_ACC_ROLE, PROPID_ACC_STATE],
            );
            NotifyWinEvent(
                EVENT_OBJECT_STATECHANGE,
                self.hwnd,
                OBJID_CLIENT,
                CHILDID_SELF as i32,
            );
        }
    }
}
