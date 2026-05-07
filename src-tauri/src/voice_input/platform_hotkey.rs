#[cfg(target_os = "macos")]
mod macos {
    use std::os::raw::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use crate::voice_input::hotkey::{
        ParsedHotkey, CARBON_CMD_KEY, CARBON_CONTROL_KEY, CARBON_OPTION_KEY, CARBON_SHIFT_KEY,
    };

    type OSStatus = i32;
    type EventHandlerCallRef = *mut c_void;
    type EventRef = *mut c_void;
    type EventTargetRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;
    type CGEventFlags = u64;
    type CGEventMask = u64;
    type CGEventRef = *mut c_void;
    type CGEventTapProxy = *mut c_void;
    type CGEventType = u32;
    type EventHandlerUPP =
        Option<unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus>;
    type CGEventTapCallBack = Option<
        unsafe extern "C" fn(CGEventTapProxy, CGEventType, CGEventRef, *mut c_void) -> CGEventRef,
    >;

    const NO_ERR: OSStatus = 0;
    const K_EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962;
    const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
    const VOICE_INPUT_HOTKEY_SIGNATURE: u32 = 0x5656_4942;
    const VOICE_INPUT_HOTKEY_ID: u32 = 1;
    const K_CG_SESSION_EVENT_TAP: u32 = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
    const K_CG_EVENT_KEY_DOWN: CGEventType = 10;
    const K_CG_EVENT_FLAGS_CHANGED: CGEventType = 12;
    const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
    const K_CG_EVENT_FLAG_MASK_SHIFT: CGEventFlags = 0x0002_0000;
    const K_CG_EVENT_FLAG_MASK_CONTROL: CGEventFlags = 0x0004_0000;
    const K_CG_EVENT_FLAG_MASK_ALTERNATE: CGEventFlags = 0x0008_0000;
    const K_CG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x0010_0000;

    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    #[repr(C)]
    struct EventHotKeyID {
        signature: u32,
        id: u32,
    }

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(
            target: EventTargetRef,
            handler: EventHandlerUPP,
            num_types: u32,
            type_list: *const EventTypeSpec,
            user_data: *mut c_void,
            handler_ref: *mut *mut c_void,
        ) -> OSStatus;
        fn RegisterEventHotKey(
            hot_key_code: u32,
            hot_key_modifiers: u32,
            hot_key_id: EventHotKeyID,
            target: EventTargetRef,
            options: u32,
            hot_key_ref: *mut EventHotKeyRef,
        ) -> OSStatus;
        fn UnregisterEventHotKey(hot_key_ref: EventHotKeyRef) -> OSStatus;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: CFStringRef;
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
        fn CFRelease(value: *const c_void);
        fn CFRunLoopAddSource(
            run_loop: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopRemoveSource(
            run_loop: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopRunInMode(
            mode: CFStringRef,
            seconds: f64,
            return_after_source_handled: u8,
        ) -> i32;
    }

    pub enum RegisteredHotkey {
        Carbon { reference: EventHotKeyRef },
        ModifierTap(ModifierTapRegistration),
    }

    pub struct ModifierTapRegistration {
        running: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    pub struct EnterSubmitRegistration {
        running: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl Drop for RegisteredHotkey {
        fn drop(&mut self) {
            match self {
                RegisteredHotkey::Carbon { reference } => {
                    if !reference.is_null() {
                        let status = unsafe { UnregisterEventHotKey(*reference) };
                        if status != NO_ERR {
                            log::warn!(
                                "Failed to unregister voice input global hotkey: OSStatus {status}"
                            );
                        }
                        *reference = ptr::null_mut();
                    }
                }
                RegisteredHotkey::ModifierTap(registration) => registration.stop(),
            }
        }
    }

    impl ModifierTapRegistration {
        fn stop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                if worker.join().is_err() {
                    log::warn!("Voice input modifier-only hotkey listener thread panicked");
                }
            }
        }
    }

    impl Drop for EnterSubmitRegistration {
        fn drop(&mut self) {
            self.stop();
        }
    }

    impl EnterSubmitRegistration {
        fn stop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                if worker.join().is_err() {
                    log::warn!("Voice input Enter submit listener thread panicked");
                }
            }
        }
    }

    pub fn install_event_handler() -> Result<(), String> {
        let event_type = EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOT_KEY_PRESSED,
        };
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                Some(hotkey_handler),
                1,
                &event_type,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status == NO_ERR {
            Ok(())
        } else {
            Err(format!("InstallEventHandler returned OSStatus {status}"))
        }
    }

    pub fn register(hotkey: &ParsedHotkey) -> Result<RegisteredHotkey, String> {
        if hotkey.is_modifier_only() {
            return register_modifier_only_hotkey(hotkey.carbon_modifiers());
        }

        let mut reference: EventHotKeyRef = ptr::null_mut();
        let status = unsafe {
            RegisterEventHotKey(
                hotkey.carbon_key_code(),
                hotkey.carbon_modifiers(),
                EventHotKeyID {
                    signature: VOICE_INPUT_HOTKEY_SIGNATURE,
                    id: VOICE_INPUT_HOTKEY_ID,
                },
                GetApplicationEventTarget(),
                0,
                &mut reference,
            )
        };
        if status == NO_ERR && !reference.is_null() {
            Ok(RegisteredHotkey::Carbon { reference })
        } else {
            Err(format!(
                "RegisterEventHotKey returned OSStatus {status}; the shortcut may be used by macOS or another app"
            ))
        }
    }

    pub fn register_enter_submit() -> Result<EnterSubmitRegistration, String> {
        let running = Arc::new(AtomicBool::new(true));
        let running_for_thread = Arc::clone(&running);
        let (ready_tx, ready_rx) = mpsc::channel();

        let worker = thread::spawn(move || {
            run_enter_submit_listener(running_for_thread, ready_tx);
        });

        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(EnterSubmitRegistration {
                running,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                running.store(false, Ordering::SeqCst);
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                running.store(false, Ordering::SeqCst);
                let _ = worker.join();
                Err("Timed out while starting Enter submit listener".to_string())
            }
        }
    }

    unsafe extern "C" fn hotkey_handler(
        _next_handler: EventHandlerCallRef,
        _event: EventRef,
        _user_data: *mut c_void,
    ) -> OSStatus {
        super::super::notify_hotkey_triggered();
        NO_ERR
    }

    fn register_modifier_only_hotkey(target_modifiers: u32) -> Result<RegisteredHotkey, String> {
        if target_modifiers == 0 {
            return Err("modifier-only hotkey must include at least one modifier".to_string());
        }

        let running = Arc::new(AtomicBool::new(true));
        let running_for_thread = Arc::clone(&running);
        let (ready_tx, ready_rx) = mpsc::channel();

        let worker = thread::spawn(move || {
            run_modifier_only_hotkey_listener(target_modifiers, running_for_thread, ready_tx);
        });

        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(RegisteredHotkey::ModifierTap(ModifierTapRegistration {
                running,
                worker: Some(worker),
            })),
            Ok(Err(error)) => {
                running.store(false, Ordering::SeqCst);
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                running.store(false, Ordering::SeqCst);
                let _ = worker.join();
                Err("Timed out while starting modifier-only hotkey listener".to_string())
            }
        }
    }

    struct ModifierTapContext {
        target_modifiers: u32,
        armed: AtomicBool,
    }

    fn run_modifier_only_hotkey_listener(
        target_modifiers: u32,
        running: Arc<AtomicBool>,
        ready_tx: mpsc::Sender<Result<(), String>>,
    ) {
        let context = Box::new(ModifierTapContext {
            target_modifiers,
            armed: AtomicBool::new(true),
        });
        let context_ptr = Box::into_raw(context);

        let tap = unsafe {
            CGEventTapCreate(
                K_CG_SESSION_EVENT_TAP,
                K_CG_HEAD_INSERT_EVENT_TAP,
                K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                1u64 << K_CG_EVENT_FLAGS_CHANGED,
                Some(modifier_only_hotkey_handler),
                context_ptr.cast::<c_void>(),
            )
        };

        if tap.is_null() {
            unsafe {
                drop(Box::from_raw(context_ptr));
            }
            let _ = ready_tx.send(Err(
                "无法监听纯修饰键快捷键，请确认辅助功能权限已授予当前应用".to_string(),
            ));
            return;
        }

        let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
        if source.is_null() {
            unsafe {
                CFRelease(tap.cast::<c_void>());
                drop(Box::from_raw(context_ptr));
            }
            let _ = ready_tx.send(Err("无法创建纯修饰键快捷键监听源".to_string()));
            return;
        }

        let run_loop = unsafe { CFRunLoopGetCurrent() };
        unsafe {
            CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
            CGEventTapEnable(tap, true);
        }
        let _ = ready_tx.send(Ok(()));

        while running.load(Ordering::SeqCst) {
            unsafe {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 1);
            }
        }

        unsafe {
            CGEventTapEnable(tap, false);
            CFRunLoopRemoveSource(run_loop, source, kCFRunLoopDefaultMode);
            CFRelease(source.cast::<c_void>());
            CFRelease(tap.cast::<c_void>());
            drop(Box::from_raw(context_ptr));
        }
    }

    fn run_enter_submit_listener(
        running: Arc<AtomicBool>,
        ready_tx: mpsc::Sender<Result<(), String>>,
    ) {
        let tap = unsafe {
            CGEventTapCreate(
                K_CG_SESSION_EVENT_TAP,
                K_CG_HEAD_INSERT_EVENT_TAP,
                K_CG_EVENT_TAP_OPTION_DEFAULT,
                1u64 << K_CG_EVENT_KEY_DOWN,
                Some(enter_submit_handler),
                ptr::null_mut(),
            )
        };

        if tap.is_null() {
            let _ = ready_tx.send(Err(
                "无法监听 Enter 结束听写，请确认辅助功能权限已授予当前应用".to_string(),
            ));
            return;
        }

        let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
        if source.is_null() {
            unsafe {
                CFRelease(tap.cast::<c_void>());
            }
            let _ = ready_tx.send(Err("无法创建 Enter 结束听写监听源".to_string()));
            return;
        }

        let run_loop = unsafe { CFRunLoopGetCurrent() };
        unsafe {
            CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
            CGEventTapEnable(tap, true);
        }
        let _ = ready_tx.send(Ok(()));

        while running.load(Ordering::SeqCst) {
            unsafe {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 1);
            }
        }

        unsafe {
            CGEventTapEnable(tap, false);
            CFRunLoopRemoveSource(run_loop, source, kCFRunLoopDefaultMode);
            CFRelease(source.cast::<c_void>());
            CFRelease(tap.cast::<c_void>());
        }
    }

    unsafe extern "C" fn enter_submit_handler(
        _proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        _user_info: *mut c_void,
    ) -> CGEventRef {
        if event_type != K_CG_EVENT_KEY_DOWN || event.is_null() {
            return event;
        }

        let key_code = CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE);
        if super::super::is_enter_key_code(key_code) && super::super::is_listening_phase() {
            super::super::notify_enter_pressed();
            return ptr::null_mut();
        }
        if super::super::is_escape_key_code(key_code) && super::super::is_listening_phase() {
            super::super::notify_escape_pressed();
            return ptr::null_mut();
        }

        event
    }

    unsafe extern "C" fn modifier_only_hotkey_handler(
        _proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        if event_type != K_CG_EVENT_FLAGS_CHANGED || user_info.is_null() {
            return event;
        }

        let context = &*(user_info as *const ModifierTapContext);
        let current_modifiers = carbon_modifiers_from_event_flags(CGEventGetFlags(event));
        let exact_match = current_modifiers == context.target_modifiers;
        let target_released = current_modifiers & context.target_modifiers == 0;

        if target_released {
            context.armed.store(true, Ordering::SeqCst);
        } else if exact_match && context.armed.swap(false, Ordering::SeqCst) {
            super::super::notify_hotkey_triggered();
        }

        event
    }

    fn carbon_modifiers_from_event_flags(flags: CGEventFlags) -> u32 {
        let mut modifiers = 0;
        if flags & K_CG_EVENT_FLAG_MASK_COMMAND != 0 {
            modifiers |= CARBON_CMD_KEY;
        }
        if flags & K_CG_EVENT_FLAG_MASK_SHIFT != 0 {
            modifiers |= CARBON_SHIFT_KEY;
        }
        if flags & K_CG_EVENT_FLAG_MASK_ALTERNATE != 0 {
            modifiers |= CARBON_OPTION_KEY;
        }
        if flags & K_CG_EVENT_FLAG_MASK_CONTROL != 0 {
            modifiers |= CARBON_CONTROL_KEY;
        }
        modifiers
    }
}

#[cfg(target_os = "macos")]
pub use macos::{install_event_handler, register, EnterSubmitRegistration, RegisteredHotkey};

#[cfg(target_os = "macos")]
pub use macos::register_enter_submit;

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use crate::voice_input::hotkey::ParsedHotkey;

    pub struct RegisteredHotkey;
    pub struct EnterSubmitRegistration;

    pub fn install_event_handler() -> Result<(), String> {
        Ok(())
    }

    pub fn register(_hotkey: &ParsedHotkey) -> Result<RegisteredHotkey, String> {
        Err("语音输入法全局快捷键 v1 仅支持 macOS".to_string())
    }

    pub fn register_enter_submit() -> Result<EnterSubmitRegistration, String> {
        Err("Enter 结束听写仅支持 macOS".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
pub use unsupported::{
    install_event_handler, register, register_enter_submit, EnterSubmitRegistration,
    RegisteredHotkey,
};
