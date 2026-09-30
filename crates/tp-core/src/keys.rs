//! Almacén global de claves de cifrado.
//!
//! El original guarda cada clave una sola vez y la reutiliza en cualquier
//! proyecto; aquí viven en `keys.toml` bajo el directorio de configuración
//! del usuario. `TEXTUREPACKER_KEYS_FILE` (o `TEXTUREPACKER_KEYS_DIR`)
//! permite apuntar a otro sitio, que es lo que usan los tests y la CI.

use crate::config::ProjectConfig;
use crate::error::{Result, TpError};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Fichero de claves: la variable de entorno manda si está definida.
pub fn keys_path() -> PathBuf {
    if let Ok(p) = std::env::var("TEXTUREPACKER_KEYS_FILE") {
        let p = p.trim();
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(dir) = std::env::var("TEXTUREPACKER_KEYS_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir).join("keys.toml");
        }
    }
    config_dir().join("keys.toml")
}

/// Directorio de configuración del usuario: XDG en Linux, APPDATA en
/// Windows y `~/.config` como respaldo.
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir).join("texturepacker-rs");
        }
    }
    if let Ok(dir) = std::env::var("APPDATA") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir).join("texturepacker-rs");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("texturepacker-rs")
}

/// Contenido del fichero de claves: `nombre -> clave`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct KeyFile {
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

fn read_file(path: &Path) -> KeyFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_file(path: &Path, file: &KeyFile) -> Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .map_err(|e| TpError::Other(format!("No se pudo crear {}: {e}", dir.display())))?;
        }
    }
    let text = toml::to_string_pretty(file)
        .map_err(|e| TpError::Other(format!("No se pudo serializar las claves: {e}")))?;
    std::fs::write(path, text)
        .map_err(|e| TpError::Other(format!("No se pudo escribir {}: {e}", path.display())))?;
    restrict(path);
    Ok(())
}

/// Permiso 600 en el fichero de claves (solo el usuario).
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

/// Nombres guardados, en orden alfabético.
pub fn list() -> Vec<String> {
    list_from(&keys_path())
}

/// Clave guardada con ese nombre.
pub fn get(name: &str) -> Option<String> {
    get_from(&keys_path(), name)
}

/// Guarda (o sobrescribe) una clave con ese nombre.
pub fn put(name: &str, key: &str) -> Result<()> {
    put_into(&keys_path(), name, key)
}

/// Borra una clave; `false` si ese nombre no existía.
pub fn remove(name: &str) -> Result<bool> {
    remove_from(&keys_path(), name)
}

/// [`list`] con la ruta explícita (tests y CI).
pub fn list_from(path: &Path) -> Vec<String> {
    read_file(path).keys.keys().cloned().collect()
}

/// [`get`] con la ruta explícita.
pub fn get_from(path: &Path, name: &str) -> Option<String> {
    read_file(path).keys.get(name).cloned()
}

/// [`put`] con la ruta explícita.
pub fn put_into(path: &Path, name: &str, key: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(TpError::Other("El nombre de la clave está vacío".into()));
    }
    if key.is_empty() {
        return Err(TpError::Other(
            "La clave a guardar está vacía; usa --key CLAVE".into(),
        ));
    }
    let mut file = read_file(path);
    file.keys.insert(name.trim().to_string(), key.to_string());
    write_file(path, &file)
}

/// [`remove`] con la ruta explícita.
pub fn remove_from(path: &Path, name: &str) -> Result<bool> {
    let mut file = read_file(path);
    let removed = file.keys.remove(name).is_some();
    if removed {
        write_file(path, &file)?;
    }
    Ok(removed)
}

/// Clave que usa una corrida: la explícita (`--key`) gana; si no hay, se
/// busca la guardada con `encryption_key_name`.
///
/// `strict` es `false` en la vista previa, que solo necesita los nombres de
/// fichero: si la clave nombrada no existe devuelve una clave vacía en lugar
/// de fallar, para que la pestaña «Archivos» siga listando lo mismo que
/// escribirá la publicación.
pub fn resolve_config(config: &ProjectConfig, strict: bool) -> Result<Cow<'_, ProjectConfig>> {
    if config.encryption_key.is_some() {
        return Ok(Cow::Borrowed(config));
    }
    let Some(name) = config.encryption_key_name.as_deref().map(str::trim) else {
        return Ok(Cow::Borrowed(config));
    };
    if name.is_empty() {
        return Ok(Cow::Borrowed(config));
    }
    match get(name) {
        Some(key) => {
            let mut resolved = config.clone();
            resolved.encryption_key = Some(key);
            Ok(Cow::Owned(resolved))
        }
        None if !strict => {
            let mut resolved = config.clone();
            resolved.encryption_key = Some(String::new());
            Ok(Cow::Owned(resolved))
        }
        None => Err(TpError::Other(format!(
            "No hay ninguna clave global llamada «{name}» (guárdala en Ajustes o en la \
             CLI con --key CLAVE --save-key {name})"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_keys(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tp_keys_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("keys.toml")
    }

    #[test]
    fn stored_keys_roundtrip_and_are_sorted() {
        let path = temp_keys("roundtrip");
        assert!(list_from(&path).is_empty());
        assert!(get_from(&path, "demo").is_none());

        put_into(&path, "demo", "clave-una").unwrap();
        put_into(&path, "otra", "clave-dos").unwrap();
        assert_eq!(
            list_from(&path),
            vec!["demo".to_string(), "otra".to_string()]
        );
        assert_eq!(get_from(&path, "demo").as_deref(), Some("clave-una"));
        // Sobrescribir no duplica.
        put_into(&path, "demo", "clave-nueva").unwrap();
        assert_eq!(list_from(&path).len(), 2);
        assert_eq!(get_from(&path, "demo").as_deref(), Some("clave-nueva"));

        assert!(remove_from(&path, "demo").unwrap());
        assert!(!remove_from(&path, "demo").unwrap());
        assert_eq!(list_from(&path), vec!["otra".to_string()]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn empty_names_and_keys_are_rejected_and_broken_files_are_ignored() {
        let path = temp_keys("invalid");
        assert!(put_into(&path, "  ", "clave").is_err());
        assert!(put_into(&path, "demo", "").is_err());

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "esto no es toml ][").unwrap();
        assert!(list_from(&path).is_empty());
        put_into(&path, "demo", "clave").unwrap();
        assert_eq!(list_from(&path).len(), 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn resolve_prefers_the_explicit_key_and_fails_on_a_missing_name() {
        let path = temp_keys("resolve");
        std::env::set_var("TEXTUREPACKER_KEYS_FILE", &path);
        put("global", "clave-global").unwrap();

        let cfg = ProjectConfig {
            encryption_key_name: Some("global".into()),
            ..ProjectConfig::default()
        };
        let resolved = resolve_config(&cfg, true).unwrap();
        assert_eq!(resolved.encryption_key.as_deref(), Some("clave-global"));

        // La clave explícita gana sobre la global.
        let cfg = ProjectConfig {
            encryption_key: Some("explicita".into()),
            ..cfg
        };
        let resolved = resolve_config(&cfg, true).unwrap();
        assert_eq!(resolved.encryption_key.as_deref(), Some("explicita"));

        // Nombre inexistente: la publicación falla, la vista previa no.
        let cfg = ProjectConfig {
            encryption_key: None,
            encryption_key_name: Some("no-existe".into()),
            ..ProjectConfig::default()
        };
        assert!(resolve_config(&cfg, true).is_err());
        let preview = resolve_config(&cfg, false).unwrap();
        assert_eq!(preview.encryption_key.as_deref(), Some(""));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
