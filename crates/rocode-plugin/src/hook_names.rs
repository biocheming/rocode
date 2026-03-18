use crate::HookEvent;

/// Map a plugin hook name string to the corresponding [`HookEvent`] variant.
///
/// Hook names follow the TypeScript/OpenCode plugin host conventions.
pub fn hook_name_to_event(name: &str) -> Option<HookEvent> {
    name.trim().parse().ok()
}
