use std::path::{Path, PathBuf};

/// Working directory layout. Portable mode is enabled when `portable.skl`
/// or an `SKL_Data` folder sits next to the executable.
#[derive(Clone, Debug)]
pub struct Paths {
    root: PathBuf,
    pub portable: bool,
}

impl Paths {
    pub fn init() -> anyhow::Result<Self> {
        let exe_dir = std::env::current_exe()?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let portable = exe_dir.join("portable.skl").exists() || exe_dir.join("SKL_Data").is_dir();
        let root = if portable {
            exe_dir.join("SKL_Data")
        } else {
            let base = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
                .unwrap_or_else(|| exe_dir.clone());
            base.join(".skl")
        };

        let paths = Self { root, portable };
        for dir in [paths.instances(), paths.java(), paths.cache(), paths.versions(), paths.libraries(), paths.assets()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(paths)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn java(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
}

/// Opens a folder, file or URL with the system's default handler.
pub fn open(target: impl AsRef<std::ffi::OsStr>) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(target).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(target).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

/// Opens a text file in an editor.
pub fn open_text(file: impl AsRef<std::ffi::OsStr>) {
    #[cfg(windows)]
    let _ = std::process::Command::new("notepad").arg(file).spawn();
    #[cfg(not(windows))]
    open(file);
}
