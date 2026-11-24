use egui::Color32;

/// Historical era of a Minecraft version. Mojang's manifest only has
/// `release / snapshot / old_beta / old_alpha`; pre-Alpha eras are
/// recovered from the id prefix (`rd-`, `c0.`, `in-`, `inf-`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Era {
    Release,
    Snapshot,
    Beta,
    Alpha,
    Infdev,
    Indev,
    Classic,
    PreClassic,
}

impl Era {
    pub fn classify(id: &str, kind: &str) -> Self {
        match kind {
            "release" => Era::Release,
            "snapshot" => Era::Snapshot,
            "old_beta" => Era::Beta,
            _ if id.starts_with("rd-") => Era::PreClassic,
            _ if id.starts_with("c0.") => Era::Classic,
            _ if id.starts_with("inf-") => Era::Infdev,
            _ if id.starts_with("in-") => Era::Indev,
            _ => Era::Alpha,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Era::Release => "Release",
            Era::Snapshot => "Snapshot",
            Era::Beta => "Beta",
            Era::Alpha => "Alpha",
            Era::Infdev => "Infdev",
            Era::Indev => "Indev",
            Era::Classic => "Classic",
            Era::PreClassic => "Pre-Classic",
        }
    }

    pub fn is_legacy(self) -> bool {
        !matches!(self, Era::Release | Era::Snapshot)
    }

    /// Muted badge colors — readable on any wallpaper, no neon.
    pub fn color(self) -> Color32 {
        match self {
            Era::Release => Color32::from_rgb(120, 190, 150),
            Era::Snapshot => Color32::from_rgb(200, 170, 110),
            Era::Beta => Color32::from_rgb(130, 170, 220),
            Era::Alpha => Color32::from_rgb(180, 150, 220),
            Era::Infdev | Era::Indev => Color32::from_rgb(210, 140, 140),
            Era::Classic | Era::PreClassic => Color32::from_rgb(170, 170, 170),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GameVersion {
    pub id: String,
    pub era: Era,
    pub url: String,
    /// `YYYY-MM-DD`
    pub date: String,
    pub year: u16,
}

pub struct VersionList {
    pub versions: Vec<GameVersion>,
    pub latest_release: String,
}

pub enum Catalog {
    Loading,
    Ready(VersionList),
    Failed(String),
}

impl Catalog {
    pub fn find(&self, id: &str) -> Option<&GameVersion> {
        match self {
            Catalog::Ready(l) => l.versions.iter().find(|v| v.id == id),
            _ => None,
        }
    }
}

impl GameVersion {
    /// Java major the version will most likely need (display only; the real
    /// component is read from the version JSON at install time).
    pub fn java_hint(&self) -> u8 {
        match self.date.as_str() {
            d if d < "2021-05-01" => 8,
            d if d < "2021-11-01" => 16,
            d if d < "2024-04-01" => 17,
            _ => 21,
        }
    }
}

impl Era {
    /// Era from the id alone (when the manifest isn't loaded yet).
    pub fn guess(id: &str) -> Era {
        let second_digit = id.chars().nth(1).is_some_and(|c| c.is_ascii_digit());
        match id.chars().next() {
            Some('b') if second_digit => Era::Beta,
            Some('a') if second_digit => Era::Alpha,
            _ if id.contains('w') && id.len() <= 7 => Era::Snapshot,
            _ => Era::classify(id, "old_alpha_or_release_guess").min_release(id),
        }
    }

    fn min_release(self, id: &str) -> Era {
        if id.starts_with("rd-") || id.starts_with("c0.") || id.starts_with("in") {
            self
        } else {
            Era::Release
        }
    }
}
