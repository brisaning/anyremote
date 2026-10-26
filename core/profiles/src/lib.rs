//! `anyremote-profiles` — Perfiles de conexión persistentes (ciclo C6, RF-04).
//!
//! Patrón H-RD-09 (clean-room D-01): catálogo de claves por ámbito + persistencia
//! TOML (D-09: `%APPDATA%\anyremote\profiles.toml`, dir sobreescriturable con
//! `ANYREMOTE_CONFIG_DIR` para tests/portabilidad).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Catálogo de claves/ámbitos (patrón H-RD-09 `KEYS_*` / `OPTION_*`).
pub mod keys {
    /// Directorio de configuración (env de override; por defecto `%APPDATA%\anyremote`).
    pub const CONFIG_DIR_ENV: &str = "ANYREMOTE_CONFIG_DIR";
    /// Archivo de perfiles dentro del directorio de configuración.
    pub const PROFILES_FILE: &str = "profiles.toml";
    /// Límite de longitud de alias (clave de identidad local del perfil).
    pub const MAX_ALIAS_LEN: usize = 64;
    /// Puerto por defecto del protocolo ARSP.
    pub const DEFAULT_PORT: u16 = 21118;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    /// Alias único (clave lógica del perfil; se sanea).
    pub id: String,
    /// Host o dirección IP.
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Preferencia de códec: "auto" | "vp9" | "deflate" | "raw".
    #[serde(default = "default_codec")]
    pub codec: String,
    /// Calidad 0..=5.
    #[serde(default = "default_quality")]
    pub quality: u8,
}

fn default_port() -> u16 {
    keys::DEFAULT_PORT
}
fn default_codec() -> String {
    "auto".into()
}
fn default_quality() -> u8 {
    3
}

impl ConnectionProfile {
    pub fn new(id: &str, host: &str, port: u16) -> Result<Self, ProfileError> {
        let p = Self {
            id: id.to_string(),
            host: host.to_string(),
            port,
            codec: default_codec(),
            quality: default_quality(),
        };
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        let alias_ok = !self.id.is_empty()
            && self.id.len() <= keys::MAX_ALIAS_LEN
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        if !alias_ok {
            return Err(ProfileError::Invalid {
                field: "id".into(),
                reason: "alias inválido (alnum/-/_ y ≤64)".into(),
            });
        }
        if self.host.trim().is_empty() {
            return Err(ProfileError::Invalid {
                field: "host".into(),
                reason: "host vacío".into(),
            });
        }
        if self.host.contains(char::is_whitespace) {
            return Err(ProfileError::Invalid {
                field: "host".into(),
                reason: "host con espacios".into(),
            });
        }
        if self.port == 0 {
            return Err(ProfileError::Invalid {
                field: "port".into(),
                reason: "puerto 0".into(),
            });
        }
        if !matches!(self.codec.as_str(), "auto" | "vp9" | "deflate" | "raw") {
            return Err(ProfileError::Invalid {
                field: "codec".into(),
                reason: format!("códec desconocido: {}", self.codec),
            });
        }
        if self.quality > 5 {
            return Err(ProfileError::Invalid {
                field: "quality".into(),
                reason: "calidad fuera de 0..=5".into(),
            });
        }
        Ok(())
    }

    /// Dirección destino para abrir una sesión (aplicación en nueva sesión).
    pub fn resolve(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, PartialEq)]
pub enum ProfileError {
    Corrupt(String),
    Invalid { field: String, reason: String },
    Io(String),
    NotFound(String),
    Duplicate(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Default, Serialize, Deserialize)]
struct ProfilesFile {
    #[serde(default)]
    profiles: Vec<ConnectionProfile>,
}

/// Almacén de perfiles persistente.
pub struct ProfilesStore {
    path: PathBuf,
}

impl ProfilesStore {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// `%APPDATA%\anyremote\profiles.toml` (o `ANYREMOTE_CONFIG_DIR`).
    pub fn default_path() -> PathBuf {
        if let Ok(dir) = std::env::var(keys::CONFIG_DIR_ENV) {
            if !dir.is_empty() {
                return PathBuf::from(dir).join(keys::PROFILES_FILE);
            }
        }
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("anyremote").join(keys::PROFILES_FILE)
    }

    pub fn default_store() -> Self {
        Self::at(Self::default_path())
    }

    /// Archivo ausente = lista vacía (no es error).
    pub fn load(&self) -> Result<Vec<ConnectionProfile>, ProfileError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&self.path).map_err(|e| ProfileError::Io(e.to_string()))?;
        let file: ProfilesFile =
            toml::from_str(&raw).map_err(|e| ProfileError::Corrupt(e.to_string()))?;
        for p in &file.profiles {
            p.validate()?;
        }
        Ok(file.profiles)
    }

    pub fn save(&self, profiles: &[ConnectionProfile]) -> Result<(), ProfileError> {
        for p in profiles {
            p.validate()?;
        }
        let mut seen = std::collections::BTreeSet::new();
        for p in profiles {
            if !seen.insert(p.id.as_str()) {
                return Err(ProfileError::Duplicate(p.id.clone()));
            }
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ProfileError::Io(e.to_string()))?;
        }
        let file = ProfilesFile {
            profiles: profiles.to_vec(),
        };
        let raw = toml::to_string_pretty(&file).map_err(|e| ProfileError::Io(e.to_string()))?;
        std::fs::write(&self.path, raw).map_err(|e| ProfileError::Io(e.to_string()))
    }

    /// Inserta o reemplaza por alias (upsert) y persiste.
    pub fn upsert(&self, profile: ConnectionProfile) -> Result<(), ProfileError> {
        let mut all = self.load()?;
        match all.iter_mut().find(|p| p.id == profile.id) {
            Some(slot) => *slot = profile,
            None => all.push(profile),
        }
        self.save(&all)
    }

    pub fn remove(&self, id: &str) -> Result<bool, ProfileError> {
        let mut all = self.load()?;
        let before = all.len();
        all.retain(|p| p.id != id);
        let removed = all.len() != before;
        if removed {
            self.save(&all)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> ProfilesStore {
        let dir = std::env::temp_dir().join(format!("anyremote-c6-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        ProfilesStore::at(dir.join(keys::PROFILES_FILE))
    }

    #[test]
    fn crud_and_persistence_across_stores() {
        let store = temp_store("crud");
        assert!(store.load().unwrap().is_empty(), "archivo ausente = vacío");

        let p = ConnectionProfile::new("oficina", "192.168.1.10", keys::DEFAULT_PORT).unwrap();
        store.upsert(p.clone()).unwrap();
        // "Nueva sesión": otra instancia del almacén sobre el mismo archivo.
        let fresh = ProfilesStore::at(store.path.clone());
        let loaded = fresh.load().unwrap();
        assert_eq!(loaded, vec![p.clone()]);

        // Update por alias.
        let mut updated = p.clone();
        updated.port = 21119;
        store.upsert(updated).unwrap();
        assert_eq!(store.load().unwrap()[0].port, 21119);

        // Remove.
        assert!(store.remove("oficina").unwrap());
        assert!(store.load().unwrap().is_empty());
        assert!(!store.remove("oficina").unwrap());
    }

    #[test]
    fn validation_rejects_bad_profiles() {
        assert!(matches!(
            ConnectionProfile::new("mal alias!", "h", 1),
            Err(ProfileError::Invalid { field, .. }) if field == "id"
        ));
        assert!(matches!(
            ConnectionProfile::new("ok", "", 1),
            Err(ProfileError::Invalid { field, .. }) if field == "host"
        ));
        assert!(matches!(
            ConnectionProfile::new("ok", "h", 0),
            Err(ProfileError::Invalid { field, .. }) if field == "port"
        ));
        let mut p = ConnectionProfile::new("ok", "h", 1).unwrap();
        p.codec = "mp4".into();
        assert!(p.validate().is_err());
        p.codec = "vp9".into();
        p.quality = 9;
        assert!(p.validate().is_err());
    }

    #[test]
    fn corrupt_file_is_an_explicit_error() {
        let store = temp_store("corrupt");
        std::fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        std::fs::write(&store.path, "esto no es toml [[[").unwrap();
        assert!(matches!(store.load(), Err(ProfileError::Corrupt(_))));
    }

    #[test]
    fn duplicate_aliases_are_rejected() {
        let store = temp_store("dup");
        let a = ConnectionProfile::new("a", "h", 1).unwrap();
        let dup = ConnectionProfile::new("a", "h2", 2).unwrap();
        assert!(matches!(
            store.save(&[a, dup]),
            Err(ProfileError::Duplicate(_))
        ));
    }

    #[test]
    fn profile_applies_to_a_new_session() {
        // Persiste un perfil apuntando a un servidor de esta prueba y una
        // "sesión nueva" (nueva instancia del almacén) lo resuelve y conecta.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let store = temp_store("apply");
        store
            .upsert(ConnectionProfile::new("local-test", "127.0.0.1", port).unwrap())
            .unwrap();

        let fresh_session = ProfilesStore::at(store.path.clone());
        let profiles = fresh_session.load().unwrap();
        let target = profiles
            .iter()
            .find(|p| p.id == "local-test")
            .expect("perfil persistido")
            .resolve();
        let conn = std::net::TcpStream::connect(target).expect("conectar usando el perfil");
        drop(conn);
        let (accept, _) = listener.accept().unwrap();
        drop(accept);
        let _ = std::fs::remove_dir_all(store.path.parent().unwrap());
    }

    #[test]
    fn default_path_honors_env_override() {
        let key = keys::CONFIG_DIR_ENV;
        let prev = std::env::var(key).ok();
        std::env::set_var(key, "/tmp/anyremote-c6-env");
        let path = ProfilesStore::default_path();
        assert!(path.to_string_lossy().contains("anyremote-c6-env"));
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
