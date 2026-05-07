use crate::types::VoiceInputInsertionResult;

pub fn replace_utf16_range(
    value: &str,
    location: usize,
    length: usize,
    replacement: &str,
) -> Result<String, String> {
    let mut units = value.encode_utf16().collect::<Vec<_>>();
    let end = location
        .checked_add(length)
        .ok_or_else(|| "selection range overflow".to_string())?;
    if location > units.len() || end > units.len() {
        return Err("selection range is outside the focused text value".to_string());
    }
    units.splice(location..end, replacement.encode_utf16());
    String::from_utf16(&units).map_err(|e| format!("replacement produced invalid UTF-16: {e}"))
}

pub fn insert_text(text: &str) -> Result<VoiceInputInsertionResult, String> {
    if text.trim().is_empty() {
        return Err("语音输入结果为空".to_string());
    }
    platform::insert_text(text)
}

pub fn should_use_ax_value_insertion(
    focused_pid: Option<i32>,
    current_pid: i32,
    focused_app_name: Option<&str>,
) -> bool {
    if focused_pid.map(|pid| pid == current_pid).unwrap_or(false) {
        return false;
    }

    focused_app_name
        .map(|name| !should_use_clipboard_paste_for_process_name(name))
        .unwrap_or(false)
}

pub fn should_use_clipboard_paste_for_process_name(name: &str) -> bool {
    !should_use_ax_value_insertion_for_process_name(name)
}

fn should_use_ax_value_insertion_for_process_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    matches!(normalized.as_str(), "textedit")
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_void};
    use std::ptr;
    use std::thread;
    use std::time::Duration;

    use super::replace_utf16_range;
    use crate::types::VoiceInputInsertionResult;
    use crate::voice_input::text as voice_text;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type AXUIElementRef = *const c_void;
    type AXValueRef = *const c_void;
    type CGEventRef = *const c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_AX_VALUE_CF_RANGE_TYPE: i32 = 4;
    const K_AX_ERROR_SUCCESS: i32 = 0;
    const K_CG_SESSION_EVENT_TAP: u32 = 1;
    const K_CG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
    const KEY_V: u16 = 9;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CFRange {
        location: isize,
        length: isize,
    }

    struct FocusedApplicationInfo {
        pid: Option<i32>,
        name: Option<String>,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> i32;
        fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> i32;
        fn AXValueGetType(value: AXValueRef) -> i32;
        fn AXValueGetValue(value: AXValueRef, value_type: i32, value_ptr: *mut c_void) -> bool;
        fn AXValueCreate(value_type: i32, value_ptr: *const c_void) -> AXValueRef;
        fn CGEventCreateKeyboardEvent(
            source: *const c_void,
            virtual_key: u16,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventPost(tap: u32, event: CGEventRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetLength(the_string: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(
            the_string: CFStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> bool;
        fn CFRelease(cf: CFTypeRef);
    }

    pub fn insert_text(text: &str) -> Result<VoiceInputInsertionResult, String> {
        let accessibility_trusted = is_accessibility_trusted();
        log::info!(
            "Voice input insert_text requested: {} accessibility_trusted={}",
            voice_text::debug_text_summary("text", text),
            accessibility_trusted
        );
        if accessibility_trusted {
            match insert_with_accessibility(text) {
                Ok(result) => return Ok(result),
                Err(error) => log::warn!("Accessibility text insertion failed: {error}"),
            }
            return paste_with_clipboard(text, true);
        }

        write_clipboard_text(text)?;
        log::info!(
            "Voice input insert_text copied because accessibility is unavailable: {}",
            voice_text::debug_text_summary("clipboard", text)
        );
        Ok(VoiceInputInsertionResult {
            strategy: "clipboard_copy".to_string(),
            inserted: false,
            clipboard_left_text: true,
            message: "缺少 Accessibility 权限，已复制，可手动粘贴".to_string(),
        })
    }

    pub fn is_accessibility_trusted() -> bool {
        unsafe { AXIsProcessTrustedWithOptions(ptr::null()) }
    }

    fn insert_with_accessibility(text: &str) -> Result<VoiceInputInsertionResult, String> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return Err("AX system element unavailable".to_string());
            }
            let focused_attr = cf_string("AXFocusedUIElement")?;
            let value_attr = cf_string("AXValue")?;
            let range_attr = cf_string("AXSelectedTextRange")?;

            let mut focused: CFTypeRef = ptr::null();
            let focused_error = AXUIElementCopyAttributeValue(system, focused_attr, &mut focused);
            let focused_app = focused_application_info(system).ok();
            release(system);
            release(focused_attr);
            if focused_error != K_AX_ERROR_SUCCESS || focused.is_null() {
                release(value_attr);
                release(range_attr);
                return Err(format!("focused AX element unavailable: {focused_error}"));
            }

            let focused_pid = focused_app
                .as_ref()
                .and_then(|app| app.pid)
                .or_else(|| focused_element_pid(focused).ok());
            let focused_app_name = focused_app.as_ref().and_then(|app| app.name.as_deref());
            let current_pid = std::process::id() as i32;
            if !super::should_use_ax_value_insertion(focused_pid, current_pid, focused_app_name) {
                let reason = if focused_pid.map(|pid| pid == current_pid).unwrap_or(false) {
                    "current process"
                } else {
                    "paste-first default"
                };
                log::info!(
                    "Voice input using clipboard paste instead of AXValue: reason={reason} focused_app={focused_app_name:?} focused_pid={focused_pid:?} current_pid={current_pid}"
                );
                release(focused);
                release(value_attr);
                release(range_attr);
                return paste_with_clipboard(text, true);
            }
            log::info!(
                "Voice input using AXValue insertion for verified native app: focused_app={focused_app_name:?} focused_pid={focused_pid:?} current_pid={current_pid}"
            );

            let mut value_ref: CFTypeRef = ptr::null();
            let value_error = AXUIElementCopyAttributeValue(focused, value_attr, &mut value_ref);
            let mut range_ref: CFTypeRef = ptr::null();
            let range_error = AXUIElementCopyAttributeValue(focused, range_attr, &mut range_ref);

            if value_error != K_AX_ERROR_SUCCESS || range_error != K_AX_ERROR_SUCCESS {
                release(focused);
                release(value_attr);
                release(range_attr);
                release(value_ref);
                release(range_ref);
                return Err(format!(
                    "focused element does not expose editable value/range: value={value_error}, range={range_error}"
                ));
            }

            let current = cf_string_to_string(value_ref as CFStringRef)?;
            let mut range = CFRange {
                location: 0,
                length: 0,
            };
            if AXValueGetType(range_ref as AXValueRef) != K_AX_VALUE_CF_RANGE_TYPE
                || !AXValueGetValue(
                    range_ref as AXValueRef,
                    K_AX_VALUE_CF_RANGE_TYPE,
                    &mut range as *mut _ as *mut c_void,
                )
            {
                release(focused);
                release(value_attr);
                release(range_attr);
                release(value_ref);
                release(range_ref);
                return Err("focused element selection range is unavailable".to_string());
            }

            let next = replace_utf16_range(
                &current,
                range.location.max(0) as usize,
                range.length.max(0) as usize,
                text,
            )?;
            log::info!(
                "Voice input AXValue replacement prepared: focused_current={} replacement={}",
                voice_text::debug_text_summary("current", &current),
                voice_text::debug_text_summary("replacement", text)
            );
            let next_ref = cf_string(&next)?;
            let set_error = AXUIElementSetAttributeValue(focused, value_attr, next_ref);
            release(next_ref);
            if set_error != K_AX_ERROR_SUCCESS {
                release(focused);
                release(value_attr);
                release(range_attr);
                release(value_ref);
                release(range_ref);
                return Err(format!("failed to set focused text value: {set_error}"));
            }

            let new_location = range.location + text.encode_utf16().count() as isize;
            let new_range = CFRange {
                location: new_location,
                length: 0,
            };
            let new_range_ref = AXValueCreate(
                K_AX_VALUE_CF_RANGE_TYPE,
                &new_range as *const _ as *const c_void,
            );
            if !new_range_ref.is_null() {
                let _ = AXUIElementSetAttributeValue(focused, range_attr, new_range_ref);
                release(new_range_ref);
            }

            release(focused);
            release(value_attr);
            release(range_attr);
            release(value_ref);
            release(range_ref);
            Ok(VoiceInputInsertionResult {
                strategy: "accessibility".to_string(),
                inserted: true,
                clipboard_left_text: false,
                message: "已写入当前输入位置".to_string(),
            })
        }
    }

    fn focused_element_pid(element: AXUIElementRef) -> Result<i32, String> {
        let mut pid = 0_i32;
        let error = unsafe { AXUIElementGetPid(element, &mut pid as *mut i32) };
        if error == K_AX_ERROR_SUCCESS {
            Ok(pid)
        } else {
            Err(format!("focused element pid unavailable: {error}"))
        }
    }

    fn focused_application_info(system: AXUIElementRef) -> Result<FocusedApplicationInfo, String> {
        let app_attr = cf_string("AXFocusedApplication")?;
        let mut focused_app: CFTypeRef = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeValue(system, app_attr, &mut focused_app) };
        release(app_attr);
        if error != K_AX_ERROR_SUCCESS || focused_app.is_null() {
            release(focused_app);
            return Err(format!("focused application unavailable: {error}"));
        }
        let pid = focused_element_pid(focused_app).ok();
        let name = ax_string_attribute(focused_app, "AXTitle").ok();
        release(focused_app);
        Ok(FocusedApplicationInfo { pid, name })
    }

    fn ax_string_attribute(element: AXUIElementRef, attribute: &str) -> Result<String, String> {
        let attr = cf_string(attribute)?;
        let mut value: CFTypeRef = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeValue(element, attr, &mut value) };
        release(attr);
        if error != K_AX_ERROR_SUCCESS || value.is_null() {
            release(value);
            return Err(format!("{attribute} unavailable: {error}"));
        }
        let result = cf_string_to_string(value as CFStringRef);
        release(value);
        result
    }

    fn paste_with_clipboard(
        text: &str,
        restore_clipboard: bool,
    ) -> Result<VoiceInputInsertionResult, String> {
        let previous_clipboard = if restore_clipboard {
            read_clipboard_text().ok()
        } else {
            None
        };
        write_clipboard_text(text)?;
        post_command_v();
        thread::sleep(Duration::from_millis(500));
        if let Some(previous) = previous_clipboard {
            let _ = write_clipboard_text(&previous);
        }
        Ok(VoiceInputInsertionResult {
            strategy: "clipboard_paste".to_string(),
            inserted: true,
            clipboard_left_text: false,
            message: "已通过剪贴板粘贴".to_string(),
        })
    }

    fn post_command_v() {
        unsafe {
            let down = CGEventCreateKeyboardEvent(ptr::null(), KEY_V, true);
            let up = CGEventCreateKeyboardEvent(ptr::null(), KEY_V, false);
            if !down.is_null() {
                CGEventSetFlags(down, K_CG_EVENT_FLAG_MASK_COMMAND);
                CGEventPost(K_CG_SESSION_EVENT_TAP, down);
                release(down);
            }
            if !up.is_null() {
                CGEventSetFlags(up, 0);
                CGEventPost(K_CG_SESSION_EVENT_TAP, up);
                release(up);
            }
        }
    }

    fn read_clipboard_text() -> Result<String, String> {
        unsafe {
            let pb = ns_pasteboard_general();
            if pb.is_null() {
                return Err("NSPasteboard unavailable".to_string());
            }
            let ns_string_type = ns_string("public.utf8-plain-text")?;
            let value = objc_msg2(pb, sel("stringForType:"), ns_string_type);
            release(ns_string_type);
            if value.is_null() {
                return Ok(String::new());
            }
            let result = cf_string_to_string(value as CFStringRef);
            result
        }
    }

    fn write_clipboard_text(text: &str) -> Result<(), String> {
        unsafe {
            let pb = ns_pasteboard_general();
            if pb.is_null() {
                return Err("NSPasteboard unavailable".to_string());
            }
            objc_msg1(pb, sel("clearContents"));
            let ns_string_type = ns_string("public.utf8-plain-text")?;
            let ns_text = ns_string(text)?;
            let ok = objc_msg3_bool(pb, sel("setString:forType:"), ns_text, ns_string_type);
            release(ns_string_type);
            release(ns_text);
            if ok {
                Ok(())
            } else {
                Err("NSPasteboard setString failed".to_string())
            }
        }
    }

    unsafe fn ns_pasteboard_general() -> *const c_void {
        let cls = objc_get_class(b"NSPasteboard\0");
        if cls.is_null() {
            return ptr::null();
        }
        objc_msg0(cls, sel("generalPasteboard"))
    }

    unsafe fn ns_string(s: &str) -> Result<*const c_void, String> {
        let cf = cf_string(s)?;
        // NSString is toll-free bridged with CFString
        Ok(cf as *const c_void)
    }

    unsafe fn objc_get_class(name: &[u8]) -> *const c_void {
        extern "C" {
            fn objc_getClass(name: *const c_char) -> *const c_void;
        }
        objc_getClass(name.as_ptr() as *const c_char)
    }

    unsafe fn sel(name: &str) -> *const c_void {
        extern "C" {
            fn sel_registerName(name: *const c_char) -> *const c_void;
        }
        let c = CString::new(name).unwrap();
        sel_registerName(c.as_ptr())
    }

    unsafe fn objc_msg0(obj: *const c_void, sel: *const c_void) -> *const c_void {
        type Fn0 = unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void;
        extern "C" {
            fn objc_msgSend(r: *const c_void, s: *const c_void, ...) -> *const c_void;
        }
        let f: Fn0 = std::mem::transmute(objc_msgSend as *const () as usize);
        f(obj, sel)
    }

    unsafe fn objc_msg1(obj: *const c_void, sel: *const c_void) {
        type Fn1 = unsafe extern "C" fn(*const c_void, *const c_void);
        extern "C" {
            fn objc_msgSend(r: *const c_void, s: *const c_void, ...) -> *const c_void;
        }
        let f: Fn1 = std::mem::transmute(objc_msgSend as *const () as usize);
        f(obj, sel)
    }

    unsafe fn objc_msg2(
        obj: *const c_void,
        sel: *const c_void,
        a1: *const c_void,
    ) -> *const c_void {
        type Fn2 =
            unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) -> *const c_void;
        extern "C" {
            fn objc_msgSend(r: *const c_void, s: *const c_void, ...) -> *const c_void;
        }
        let f: Fn2 = std::mem::transmute(objc_msgSend as *const () as usize);
        f(obj, sel, a1)
    }

    unsafe fn objc_msg3_bool(
        obj: *const c_void,
        sel: *const c_void,
        a1: *const c_void,
        a2: *const c_void,
    ) -> bool {
        type Fn3 =
            unsafe extern "C" fn(*const c_void, *const c_void, *const c_void, *const c_void) -> u8;
        extern "C" {
            fn objc_msgSend(r: *const c_void, s: *const c_void, ...) -> *const c_void;
        }
        let f: Fn3 = std::mem::transmute(objc_msgSend as *const () as usize);
        f(obj, sel, a1, a2) != 0
    }

    fn cf_string(value: &str) -> Result<CFStringRef, String> {
        let c_value =
            CString::new(value).map_err(|_| "CFString cannot contain NUL bytes".to_string())?;
        let string_ref = unsafe {
            CFStringCreateWithCString(ptr::null(), c_value.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        };
        if string_ref.is_null() {
            Err("failed to create CFString".to_string())
        } else {
            Ok(string_ref)
        }
    }

    fn cf_string_to_string(value: CFStringRef) -> Result<String, String> {
        if value.is_null() {
            return Err("CFString is null".to_string());
        }
        unsafe {
            let length = CFStringGetLength(value);
            let max_size = CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8) + 1;
            let mut buffer = vec![0_i8; max_size as usize];
            if !CFStringGetCString(
                value,
                buffer.as_mut_ptr(),
                max_size,
                K_CF_STRING_ENCODING_UTF8,
            ) {
                return Err("failed to read CFString as UTF-8".to_string());
            }
            let bytes = buffer
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| *byte as u8)
                .collect::<Vec<_>>();
            String::from_utf8(bytes).map_err(|e| format!("CFString was not UTF-8: {e}"))
        }
    }

    fn release(value: CFTypeRef) {
        if !value.is_null() {
            unsafe { CFRelease(value) };
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use crate::types::VoiceInputInsertionResult;

    pub fn insert_text(_text: &str) -> Result<VoiceInputInsertionResult, String> {
        Err("语音输入法 v1 仅支持 macOS".to_string())
    }
}
