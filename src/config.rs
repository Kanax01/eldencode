use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn discover(cli_home: Option<&str>) -> Paths {
        let home = if let Some(h) = cli_home.filter(|h| !h.trim().is_empty()) {
            expand(h)
        } else if let Ok(env_home) = std::env::var("ELDENCODE_HOME") {
            if env_home.trim().is_empty() {
                default_home()
            } else {
                expand(&env_home)
            }
        } else {
            default_home()
        };
        Paths { home }
    }

    pub fn config(&self) -> PathBuf {
        self.home.join("config.toml")
    }

    pub fn backups(&self) -> PathBuf {
        self.home.join("backups")
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn default_home() -> PathBuf {
    home_dir().join(".eldencode")
}

pub fn expand(raw: &str) -> PathBuf {
    let raw = raw.trim();
    if raw == "~" {
        home_dir()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(raw)
    }
}

pub fn display(path: &Path) -> String {
    match path.strip_prefix(home_dir()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    pub animations: bool,

    pub process: String,
    pub auto_attach: bool,
    pub backup_on_attach: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: "erdtree".to_string(),
            animations: true,
            process: "eldenring.exe".to_string(),
            auto_attach: true,
            backup_on_attach: true,
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Config {
        fs::read_to_string(paths.config())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> Result<(), String> {
        let text =
            toml::to_string_pretty(self).map_err(|err| format!("cannot encode config: {err}"))?;
        fs::write(paths.config(), text)
            .map_err(|err| format!("cannot write {}: {err}", display(&paths.config())))
    }
}

pub fn ensure_home(paths: &Paths) -> io::Result<()> {
    fs::create_dir_all(&paths.home)?;
    fs::create_dir_all(paths.backups())?;
    if !paths.config().exists() {
        let _ = Config::default().save(paths);
    }
    Ok(())
}
