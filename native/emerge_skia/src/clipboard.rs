#[cfg(all(feature = "macos", target_os = "macos"))]
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
#[cfg(all(feature = "macos", target_os = "macos"))]
use objc2_foundation::NSString;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClipboardTarget {
    Clipboard,
    Primary,
}

pub struct ClipboardManager {
    system_enabled: bool,
    #[cfg(all(feature = "wayland", target_os = "linux"))]
    system: Option<arboard::Clipboard>,
    fallback_clipboard: String,
    fallback_primary: String,
}

impl ClipboardManager {
    pub fn new(system_enabled: bool) -> Self {
        Self {
            system_enabled,
            #[cfg(all(feature = "wayland", target_os = "linux"))]
            system: None,
            fallback_clipboard: String::new(),
            fallback_primary: String::new(),
        }
    }

    pub fn set_text(&mut self, target: ClipboardTarget, text: &str) {
        self.set_fallback(target, text);

        if self.system_enabled {
            #[cfg(all(feature = "wayland", target_os = "linux"))]
            if let Some(system) = self.system_mut() {
                let _ = set_system_text(system, target, text);
            }

            #[cfg(all(feature = "macos", target_os = "macos"))]
            {
                let _ = set_system_text(target, text);
            }
        }
    }

    pub fn get_text(&mut self, target: ClipboardTarget) -> Option<String> {
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        if self.system_enabled
            && let Some(system) = self.system_mut()
            && let Ok(text) = get_system_text(system, target)
        {
            self.set_fallback(target, &text);
            return if text.is_empty() { None } else { Some(text) };
        }

        #[cfg(all(feature = "macos", target_os = "macos"))]
        if self.system_enabled
            && let Ok(text) = get_system_text(target)
        {
            self.set_fallback(target, &text);
            return if text.is_empty() { None } else { Some(text) };
        }

        self.get_fallback(target)
    }

    #[cfg(all(feature = "wayland", target_os = "linux"))]
    fn system_mut(&mut self) -> Option<&mut arboard::Clipboard> {
        if !self.system_enabled {
            return None;
        }

        if self.system.is_none() {
            self.system = arboard::Clipboard::new().ok();
        }

        self.system.as_mut()
    }

    fn set_fallback(&mut self, target: ClipboardTarget, text: &str) {
        match target {
            ClipboardTarget::Clipboard => {
                self.fallback_clipboard.clear();
                self.fallback_clipboard.push_str(text);
            }
            ClipboardTarget::Primary => {
                self.fallback_primary.clear();
                self.fallback_primary.push_str(text);
            }
        }
    }

    fn get_fallback(&self, target: ClipboardTarget) -> Option<String> {
        let value = match target {
            ClipboardTarget::Clipboard => &self.fallback_clipboard,
            ClipboardTarget::Primary => &self.fallback_primary,
        };

        if value.is_empty() {
            None
        } else {
            Some(value.clone())
        }
    }
}

#[cfg(all(feature = "wayland", target_os = "linux"))]
fn linux_clipboard_kind(target: ClipboardTarget) -> arboard::LinuxClipboardKind {
    match target {
        ClipboardTarget::Clipboard => arboard::LinuxClipboardKind::Clipboard,
        ClipboardTarget::Primary => arboard::LinuxClipboardKind::Primary,
    }
}

#[cfg(all(feature = "wayland", target_os = "linux"))]
fn set_system_text(
    system: &mut arboard::Clipboard,
    target: ClipboardTarget,
    text: &str,
) -> Result<(), String> {
    use arboard::SetExtLinux;

    system
        .set()
        .clipboard(linux_clipboard_kind(target))
        .text(text.to_string())
        .map_err(|err| err.to_string())
}

#[cfg(all(feature = "wayland", target_os = "linux"))]
fn get_system_text(
    system: &mut arboard::Clipboard,
    target: ClipboardTarget,
) -> Result<String, String> {
    use arboard::GetExtLinux;

    system
        .get()
        .clipboard(linux_clipboard_kind(target))
        .text()
        .map_err(|err| err.to_string())
}

#[cfg(all(feature = "macos", target_os = "macos"))]
fn set_system_text(target: ClipboardTarget, text: &str) -> Result<(), String> {
    if target != ClipboardTarget::Clipboard {
        return Err("primary selection is not supported on macOS".to_string());
    }

    let pasteboard = NSPasteboard::generalPasteboard();
    let text = NSString::from_str(text);
    pasteboard.clearContents();
    let pasteboard_type = unsafe { NSPasteboardTypeString };

    if pasteboard.setString_forType(&text, pasteboard_type) {
        Ok(())
    } else {
        Err("failed to write macOS pasteboard string".to_string())
    }
}

#[cfg(all(feature = "macos", target_os = "macos"))]
fn get_system_text(target: ClipboardTarget) -> Result<String, String> {
    if target != ClipboardTarget::Clipboard {
        return Err("primary selection is not supported on macOS".to_string());
    }

    let pasteboard = NSPasteboard::generalPasteboard();
    let pasteboard_type = unsafe { NSPasteboardTypeString };
    pasteboard
        .stringForType(pasteboard_type)
        .map(|value| value.to_string())
        .ok_or_else(|| "failed to read macOS pasteboard string".to_string())
}

#[cfg(test)]
mod tests {
    use super::{ClipboardManager, ClipboardTarget};

    #[test]
    fn fallback_clipboard_roundtrip_when_system_disabled() {
        let mut manager = ClipboardManager::new(false);
        manager.set_text(ClipboardTarget::Clipboard, "copy value");

        let pasted = manager.get_text(ClipboardTarget::Clipboard);
        assert_eq!(pasted.as_deref(), Some("copy value"));
    }

    #[test]
    fn fallback_primary_roundtrip_when_system_disabled() {
        let mut manager = ClipboardManager::new(false);
        manager.set_text(ClipboardTarget::Primary, "primary value");

        let pasted = manager.get_text(ClipboardTarget::Primary);
        assert_eq!(pasted.as_deref(), Some("primary value"));
    }

    #[test]
    fn empty_fallback_returns_none() {
        let manager = ClipboardManager::new(false);

        assert_eq!(manager.get_fallback(ClipboardTarget::Clipboard), None);
        assert_eq!(manager.get_fallback(ClipboardTarget::Primary), None);
    }
}
