use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::i18n::Lang;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePreset {
    #[default]
    FrostedGlass,
    DarkOled,
    ClassicStone,
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 3] = [Self::FrostedGlass, Self::DarkOled, Self::ClassicStone];

    pub fn name(self) -> &'static str {
        match self {
            Self::FrostedGlass => "Frosted Glass",
            Self::DarkOled => "Dark OLED",
            Self::ClassicStone => "Classic Stone",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WallpaperSource {
    Builtin(u8),
    Custom(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Lang,
    pub preset: ThemePreset,
    pub accent: [u8; 3],
    pub blur: f32,
    pub dim: f32,
    pub wallpaper: WallpaperSource,
    pub nickname: String,
    pub selected_instance: Option<String>,
    pub ui_scale: f32,
    pub reduce_motion: bool,
    pub memory_mb: u32,
    pub curseforge_key: String,
    pub skin_slim: bool,
    /// Public Mojang texture URL of the current skin (enables `/skin url` for everyone).
    pub skin_url: Option<String>,
    /// Glass panel opacity multiplier (0.5 = see-through, 1.4 = solid).
    pub panel_opacity: f32,
    /// Animation pace: 0 fast, 1 normal, 2 smooth.
    pub anim_speed: u8,
    pub after_launch: AfterLaunch,
    pub restore_on_exit: bool,
    /// Zip changed worlds before every launch.
    pub backup_worlds: bool,
    pub backup_keep: u32,
    pub game: GameOpts,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::Uk,
            preset: ThemePreset::FrostedGlass,
            accent: [124, 140, 255],
            blur: 18.0,
            dim: 0.35,
            wallpaper: WallpaperSource::Builtin(0),
            nickname: "Player".into(),
            selected_instance: None,
            ui_scale: 1.0,
            reduce_motion: false,
            memory_mb: 4096,
            curseforge_key: String::new(),
            skin_slim: false,
            skin_url: None,
            panel_opacity: 1.0,
            anim_speed: 1,
            after_launch: AfterLaunch::Keep,
            restore_on_exit: true,
            backup_worlds: true,
            backup_keep: 5,
            game: GameOpts::default(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

impl Settings {
    /// Duration multiplier for animations (0 = motion off).
    pub fn motion_scale(&self) -> f32 {
        if self.reduce_motion {
            return 0.0;
        }
        match self.anim_speed {
            0 => 0.65,
            2 => 1.5,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AfterLaunch {
    #[default]
    Keep,
    Minimize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JvmPreset {
    #[default]
    Default,
    /// Aikar's tuned G1 flags — fewer GC stutters on modpacks.
    G1,
    /// Generational ZGC (Java 17+; falls back to G1 on Java 8).
    Zgc,
}

/// Per-launch game options, applied on top of the version's own arguments.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameOpts {
    /// 0 = let the game decide.
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    /// `host[:port]` to join right after the game loads.
    pub server: String,
    pub jvm_preset: JvmPreset,
    pub jvm_args: String,
    pub game_args: String,
    /// Custom java(w).exe; empty = Mojang runtime.
    pub java_path: String,
}

impl GameOpts {
    pub fn preset_flags(&self, java8: bool) -> Vec<String> {
        const G1: &[&str] = &[
            "-XX:+UseG1GC", "-XX:+ParallelRefProcEnabled", "-XX:MaxGCPauseMillis=200",
            "-XX:+UnlockExperimentalVMOptions", "-XX:+DisableExplicitGC", "-XX:G1NewSizePercent=30",
            "-XX:G1MaxNewSizePercent=40", "-XX:G1HeapRegionSize=8M", "-XX:G1ReservePercent=20",
            "-XX:InitiatingHeapOccupancyPercent=15",
        ];
        let v: &[&str] = match self.jvm_preset {
            JvmPreset::Default => &[],
            JvmPreset::Zgc if !java8 => &["-XX:+UseZGC", "-XX:+ZGenerational"],
            _ => G1,
        };
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Splits `host:port`; port defaults to 25565.
    pub fn server_addr(&self) -> Option<(String, u16)> {
        let s = self.server.trim();
        if s.is_empty() {
            return None;
        }
        match s.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => Some((h.to_string(), p.parse().unwrap())),
            _ => Some((s.to_string(), 25565)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn game_opts() {
        let mut g = GameOpts { server: "mc.example.org:25570".into(), ..Default::default() };
        assert_eq!(g.server_addr(), Some(("mc.example.org".into(), 25570)));
        g.server = " play.x ".into();
        assert_eq!(g.server_addr(), Some(("play.x".into(), 25565)));
        g.jvm_preset = JvmPreset::Zgc;
        assert!(g.preset_flags(true).contains(&"-XX:+UseG1GC".to_string()));
        assert!(g.preset_flags(false).contains(&"-XX:+UseZGC".to_string()));
        // old configs without the new fields still load
        let s: Settings = serde_json::from_str(r#"{"nickname":"Abc"}"#).unwrap();
        assert_eq!(s.nickname, "Abc");
        assert_eq!(s.panel_opacity, 1.0);
    }
}
