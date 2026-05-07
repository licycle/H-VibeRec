use crate::audio::{default_output_device, supports_system_audio};
use log::{error, info};
use serde::Serialize;
use std::process::Command;

#[derive(Serialize)]
pub struct AudioPermissionStatus {
    pub platform: String,
    pub microphone_ok: bool,
    pub microphone_message: String,
    pub system_audio_ok: bool,
    pub system_audio_message: String,
    pub opened_settings: bool,
    pub settings_message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PermissionSettingsTarget {
    Microphone,
    SystemAudio,
    Accessibility,
    General,
}

#[tauri::command]
pub async fn request_microphone_permission() -> Result<(), String> {
    info!("Requesting microphone permission...");

    match crate::audio::trigger_audio_permission() {
        Ok(_) => {
            info!("Microphone permission request completed");
            Ok(())
        }
        Err(e) => {
            error!("Failed to request microphone permission: {}", e);
            Err(format!("Failed to request microphone permission: {}", e))
        }
    }
}

#[tauri::command]
pub async fn verify_audio_permissions(
    open_settings_on_failure: bool,
) -> Result<AudioPermissionStatus, String> {
    info!("Verifying audio permissions...");

    let platform = std::env::consts::OS.to_string();
    let microphone_result = crate::audio::trigger_audio_permission();
    let system_audio_result = crate::audio::trigger_system_audio_permission().await;
    let microphone_ok = microphone_result.is_ok();
    let system_audio_ok = system_audio_result.is_ok();
    let should_open_settings = open_settings_on_failure && (!microphone_ok || !system_audio_ok);
    let (opened_settings, settings_message) = if should_open_settings {
        open_audio_privacy_settings(failed_permission_target(microphone_ok, system_audio_ok))
    } else {
        (false, platform_settings_hint(&platform).to_string())
    };

    let status = AudioPermissionStatus {
        platform,
        microphone_ok,
        microphone_message: match microphone_result {
            Ok(()) => "麦克风权限可用".to_string(),
            Err(error) => format_permission_error("麦克风权限不可用", error),
        },
        system_audio_ok,
        system_audio_message: match system_audio_result {
            Ok(()) => "系统音频捕捉权限可用".to_string(),
            Err(error) => format_permission_error("系统音频捕捉权限不可用", error),
        },
        opened_settings,
        settings_message,
    };

    info!(
        "Audio permission verification complete: mic={}, system_audio={}, opened_settings={}",
        status.microphone_ok, status.system_audio_ok, status.opened_settings
    );
    Ok(status)
}

#[tauri::command]
pub async fn open_audio_permission_settings(target: Option<String>) -> Result<String, String> {
    let settings_target = permission_settings_target(target.as_deref());
    let (opened, message) = open_audio_privacy_settings(settings_target);
    if opened {
        Ok(message)
    } else {
        Err(message)
    }
}

fn permission_settings_target(target: Option<&str>) -> PermissionSettingsTarget {
    match target {
        Some("microphone") => PermissionSettingsTarget::Microphone,
        Some("system_audio") => PermissionSettingsTarget::SystemAudio,
        Some("accessibility") => PermissionSettingsTarget::Accessibility,
        _ => PermissionSettingsTarget::General,
    }
}

#[tauri::command]
pub async fn check_audio_devices() -> Result<String, String> {
    let mut status = String::new();

    // Check default input device
    match crate::audio::default_input_device() {
        Ok(device) => {
            status.push_str(&format!("✓ Default Input: {}\n", device.name));
        }
        Err(e) => {
            status.push_str(&format!("✗ Default Input: {}\n", e));
        }
    }

    // Check default output device for system audio
    match default_output_device() {
        Ok(device) => {
            let supports_system = supports_system_audio(&device);
            status.push_str(&format!(
                "✓ Default Output: {} (System Audio: {})\n",
                device.name,
                if supports_system { "✓" } else { "✗" }
            ));
        }
        Err(e) => {
            status.push_str(&format!("✗ Default Output: {}\n", e));
        }
    }

    // List all available devices
    match crate::audio::list_audio_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                status.push_str("\nNo audio devices found\n");
            } else {
                status.push_str(&format!("\nAvailable devices ({}):\n", devices.len()));
                for device in devices {
                    let system_support = if device.device_type == crate::audio::DeviceType::Output {
                        if supports_system_audio(&device) {
                            " [System Audio ✓]"
                        } else {
                            " [System Audio ✗]"
                        }
                    } else {
                        ""
                    };
                    status.push_str(&format!("  - {}{}\n", device, system_support));
                }
            }
        }
        Err(e) => {
            status.push_str(&format!("\nError listing devices: {}\n", e));
        }
    }

    Ok(status)
}

fn format_permission_error(prefix: &str, error: anyhow::Error) -> String {
    let message = error.to_string();
    if message.contains("TCC") || message.to_lowercase().contains("permission") {
        format!("{prefix}：系统隐私权限未授权或已被拒绝")
    } else {
        format!("{prefix}：{message}")
    }
}

fn failed_permission_target(
    microphone_ok: bool,
    system_audio_ok: bool,
) -> PermissionSettingsTarget {
    if !microphone_ok {
        PermissionSettingsTarget::Microphone
    } else if !system_audio_ok {
        PermissionSettingsTarget::SystemAudio
    } else {
        PermissionSettingsTarget::General
    }
}

fn open_audio_privacy_settings(target: PermissionSettingsTarget) -> (bool, String) {
    match std::env::consts::OS {
        "macos" => open_macos_privacy_settings(target),
        "windows" => open_windows_privacy_settings(target),
        platform => (
            false,
            format!("当前平台 {platform} 不支持自动打开权限设置，请在系统隐私设置里检查麦克风权限"),
        ),
    }
}

#[cfg(target_os = "macos")]
fn open_macos_privacy_settings(target: PermissionSettingsTarget) -> (bool, String) {
    let urls = macos_privacy_settings_urls(target);

    for (url, message) in urls {
        if Command::new("open")
            .arg(url)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return (true, message.to_string());
        }
    }

    (false, macos_manual_settings_hint(target).to_string())
}

#[cfg(target_os = "macos")]
fn macos_privacy_settings_urls(
    target: PermissionSettingsTarget,
) -> &'static [(&'static str, &'static str)] {
    match target {
        PermissionSettingsTarget::Microphone => &[
            (
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
                "已打开 macOS 麦克风隐私设置，请允许 H-VibeRec",
            ),
            (
                "x-apple.systempreferences:com.apple.preference.security",
                "已打开 macOS 隐私与安全性设置，请检查麦克风权限",
            ),
        ],
        PermissionSettingsTarget::SystemAudio => &[
            (
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                "已打开 macOS 屏幕与系统音频录制设置，请允许 H-VibeRec",
            ),
            (
                "x-apple.systempreferences:com.apple.preference.security",
                "已打开 macOS 隐私与安全性设置，请检查屏幕与系统音频录制",
            ),
        ],
        PermissionSettingsTarget::Accessibility => &[
            (
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                "已打开 macOS 辅助功能隐私设置，请允许 H-VibeRec",
            ),
            (
                "x-apple.systempreferences:com.apple.preference.security",
                "已打开 macOS 隐私与安全性设置，请检查辅助功能权限",
            ),
        ],
        PermissionSettingsTarget::General => &[(
            "x-apple.systempreferences:com.apple.preference.security",
            "已打开 macOS 隐私与安全性设置，请检查麦克风和屏幕与系统音频录制",
        )],
    }
}

#[cfg(not(target_os = "macos"))]
fn open_macos_privacy_settings(_target: PermissionSettingsTarget) -> (bool, String) {
    (
        false,
        platform_settings_hint(std::env::consts::OS).to_string(),
    )
}

#[cfg(target_os = "windows")]
fn open_windows_privacy_settings(target: PermissionSettingsTarget) -> (bool, String) {
    let targets: &[(&str, &str)] = match target {
        PermissionSettingsTarget::Microphone => &[
            (
                "ms-settings:privacy-microphone",
                "已打开 Windows 麦克风隐私设置，请允许桌面应用访问麦克风",
            ),
            (
                "ms-settings:privacy",
                "已打开 Windows 隐私设置，请检查麦克风权限",
            ),
        ],
        PermissionSettingsTarget::SystemAudio => &[(
            "ms-settings:sound",
            "已打开 Windows 声音设置，请检查输入/输出设备",
        )],
        PermissionSettingsTarget::Accessibility => &[(
            "ms-settings:privacy",
            "已打开 Windows 隐私设置；语音输入自动写入当前仅支持 macOS Accessibility",
        )],
        PermissionSettingsTarget::General => &[(
            "ms-settings:privacy-microphone",
            "已打开 Windows 麦克风隐私设置，请允许桌面应用访问麦克风",
        )],
    };

    for (settings_uri, message) in targets {
        let result = Command::new("cmd")
            .args(["/C", "start", "", settings_uri])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if result {
            return (true, message.to_string());
        }
    }

    (false, windows_manual_settings_hint(target).to_string())
}

#[cfg(not(target_os = "windows"))]
fn open_windows_privacy_settings(_target: PermissionSettingsTarget) -> (bool, String) {
    (
        false,
        platform_settings_hint(std::env::consts::OS).to_string(),
    )
}

#[cfg(target_os = "macos")]
fn macos_manual_settings_hint(target: PermissionSettingsTarget) -> &'static str {
    match target {
        PermissionSettingsTarget::Microphone => {
            "无法自动打开 macOS 麦克风设置，请手动进入 系统设置 > 隐私与安全性 > 麦克风"
        }
        PermissionSettingsTarget::SystemAudio => {
            "无法自动打开 macOS 系统音频设置，请手动进入 系统设置 > 隐私与安全性 > 屏幕与系统音频录制"
        }
        PermissionSettingsTarget::Accessibility => {
            "无法自动打开 macOS 辅助功能设置，请手动进入 系统设置 > 隐私与安全性 > 辅助功能"
        }
        PermissionSettingsTarget::General => {
            "无法自动打开 macOS 权限设置，请手动进入 系统设置 > 隐私与安全性 > 麦克风 / 屏幕与系统音频录制"
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_manual_settings_hint(target: PermissionSettingsTarget) -> &'static str {
    match target {
        PermissionSettingsTarget::Microphone => {
            "无法自动打开 Windows 麦克风设置，请手动进入 设置 > 隐私和安全性 > 麦克风"
        }
        PermissionSettingsTarget::SystemAudio => {
            "无法自动打开 Windows 声音设置，请手动进入 设置 > 系统 > 声音"
        }
        PermissionSettingsTarget::Accessibility => {
            "当前平台不支持 macOS Accessibility，请手动检查系统辅助功能或隐私设置"
        }
        PermissionSettingsTarget::General => {
            "无法自动打开 Windows 权限设置，请手动进入 设置 > 隐私和安全性 > 麦克风"
        }
    }
}

fn platform_settings_hint(platform: &str) -> &'static str {
    match platform {
        "macos" => "macOS：系统设置 > 隐私与安全性 > 麦克风 / 屏幕与系统音频录制",
        "windows" => "Windows：设置 > 隐私和安全性 > 麦克风，并允许桌面应用访问麦克风",
        _ => "请在系统隐私设置中检查音频输入权限",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_settings_target_accepts_accessibility() {
        assert_eq!(
            permission_settings_target(Some("accessibility")),
            PermissionSettingsTarget::Accessibility
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_accessibility_target_opens_accessibility_privacy_url() {
        let urls = macos_privacy_settings_urls(PermissionSettingsTarget::Accessibility);

        assert!(urls
            .first()
            .map(|(url, _)| url.contains("Privacy_Accessibility"))
            .unwrap_or(false));
        assert!(
            macos_manual_settings_hint(PermissionSettingsTarget::Accessibility)
                .contains("辅助功能")
        );
    }
}
