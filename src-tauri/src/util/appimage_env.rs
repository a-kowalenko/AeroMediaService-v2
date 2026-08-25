//! Undo AppImage runtime environment mutations before spawning host children.
//!
//! When running from an AppImage, `LD_LIBRARY_PATH`, `PATH`, GTK/GIO module caches,
//! and related variables point into the transient squashfs mount. Those values are
//! correct only for binaries inside the bundle. Children outside it — browsers via
//! `xdg-open`, file managers, etc. — load bundle libraries and typically fail silently.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// `(key, Some(new_value))` → set; `(key, None)` → remove.
pub type EnvOverride = (&'static str, Option<String>);

/// Colon-separated search-path variables: strip entries under the mount, keep the rest.
const LIST_VARS: &[&str] = &[
    "PATH",
    "LD_LIBRARY_PATH",
    "XDG_DATA_DIRS",
    "GTK_PATH",
    "GIO_EXTRA_MODULES",
    "GSETTINGS_SCHEMA_DIR",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "QT_PLUGIN_PATH",
    "PYTHONPATH",
    "PERLLIB",
];

/// Single-path variables: drop entirely when they point into the mount.
const PREFIX_VARS: &[&str] = &[
    "GDK_PIXBUF_MODULE_FILE",
    "GDK_PIXBUF_MODULE_DIR",
    "GTK_IM_MODULE_FILE",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "PYTHONHOME",
];

/// AppRun exports these unconditionally; AppImage identity vars confuse host tools.
const ALWAYS_REMOVE: &[&str] = &[
    "GDK_BACKEND",
    "GTK_THEME",
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "LD_PRELOAD",
];

/// Env fixes to apply to a child spawn. Empty unless running from a Linux AppImage.
pub fn sanitized_env_overrides() -> Vec<EnvOverride> {
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
    #[cfg(target_os = "linux")]
    {
        match std::env::var("APPDIR") {
            Ok(appdir) if !appdir.trim().is_empty() => {
                overrides_for(&appdir, |key| std::env::var(key).ok())
            }
            _ => Vec::new(),
        }
    }
}

/// Apply [`sanitized_env_overrides`] to a `std::process::Command`.
pub fn sanitize_std_command(cmd: &mut std::process::Command) {
    for (key, value) in sanitized_env_overrides() {
        match value {
            Some(v) => {
                cmd.env(key, v);
            }
            None => {
                cmd.env_remove(key);
            }
        }
    }
}

fn overrides_for(appdir: &str, get: impl Fn(&str) -> Option<String>) -> Vec<EnvOverride> {
    let mut overrides: Vec<EnvOverride> = Vec::new();
    for &var in LIST_VARS {
        let Some(value) = get(var) else { continue };
        let kept: Vec<&str> = value
            .split(':')
            .filter(|entry| !entry.is_empty() && !path_is_under(entry, appdir))
            .collect();
        let filtered = kept.join(":");
        if filtered != value {
            overrides.push((var, (!filtered.is_empty()).then_some(filtered)));
        }
    }
    for &var in PREFIX_VARS {
        if get(var).is_some_and(|value| path_is_under(&value, appdir)) {
            overrides.push((var, None));
        }
    }
    for &var in ALWAYS_REMOVE {
        if get(var).is_some() {
            overrides.push((var, None));
        }
    }
    overrides
}

fn path_is_under(path: &str, appdir: &str) -> bool {
    let base = appdir.trim_end_matches('/');
    if base.is_empty() {
        return false;
    }
    let candidate = path.trim_end_matches('/');
    candidate == base
        || candidate
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const APPDIR: &str = "/tmp/.mount_AeroXYZ";

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn run(vars: &HashMap<String, String>) -> HashMap<&'static str, Option<String>> {
        overrides_for(APPDIR, |key| vars.get(key).cloned())
            .into_iter()
            .collect()
    }

    #[test]
    fn strips_mount_entries_but_keeps_user_entries() {
        let vars = env(&[(
            "PATH",
            "/tmp/.mount_AeroXYZ/usr/bin/:/tmp/.mount_AeroXYZ/usr/sbin/:/usr/local/bin:/usr/bin",
        )]);
        let out = run(&vars);
        assert_eq!(
            out.get("PATH"),
            Some(&Some("/usr/local/bin:/usr/bin".to_string()))
        );
    }

    #[test]
    fn removes_var_when_only_mount_entries_remain() {
        let vars = env(&[(
            "LD_LIBRARY_PATH",
            "/tmp/.mount_AeroXYZ/usr/lib/:/tmp/.mount_AeroXYZ/lib/x86_64-linux-gnu/:",
        )]);
        let out = run(&vars);
        assert_eq!(out.get("LD_LIBRARY_PATH"), Some(&None));
    }

    #[test]
    fn untouched_list_var_is_not_overridden() {
        let vars = env(&[("PATH", "/usr/local/bin:/usr/bin")]);
        assert!(run(&vars).is_empty());
    }

    #[test]
    fn double_slash_hook_paths_are_recognized() {
        let vars = env(&[(
            "GTK_PATH",
            "/tmp/.mount_AeroXYZ//usr/lib/x86_64-linux-gnu/gtk-3.0:/usr/lib/x86_64-linux-gnu/gtk-3.0",
        )]);
        let out = run(&vars);
        assert_eq!(
            out.get("GTK_PATH"),
            Some(&Some("/usr/lib/x86_64-linux-gnu/gtk-3.0".to_string()))
        );
    }

    #[test]
    fn forced_and_identity_vars_are_removed() {
        let vars = env(&[
            ("GDK_BACKEND", "x11"),
            ("GTK_THEME", "Adwaita:light"),
            ("APPDIR", APPDIR),
            ("APPIMAGE", "/home/user/AeroMediaService.AppImage"),
            ("LD_PRELOAD", "/tmp/.mount_AeroXYZ/lib/preload.so"),
        ]);
        let out = run(&vars);
        assert_eq!(out.get("GDK_BACKEND"), Some(&None));
        assert_eq!(out.get("GTK_THEME"), Some(&None));
        assert_eq!(out.get("APPDIR"), Some(&None));
        assert_eq!(out.get("APPIMAGE"), Some(&None));
        assert_eq!(out.get("LD_PRELOAD"), Some(&None));
    }

    #[test]
    fn sibling_mount_prefix_is_not_confused_with_the_mount() {
        assert!(!path_is_under("/tmp/.mount_AeroXYZ2/usr/lib", APPDIR));
        assert!(path_is_under("/tmp/.mount_AeroXYZ/usr/lib", APPDIR));
    }
}
