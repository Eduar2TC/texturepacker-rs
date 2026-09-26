//! Tipos de error del motor (`TpError`) y alias `Result` de la crate.
//!
//! La librería usa [`thiserror`] para modelar los fallos del pipeline; los
//! mensajes originales (en español) se conservan en los atributos
//! `#[error("…")]` y en las variantes con texto propio.

use thiserror::Error;

/// Error del motor TexturePacker-RS.
#[derive(Debug, Error)]
pub enum TpError {
    /// Entrada/salida de disco.
    #[error("Error de E/S: {0}")]
    Io(#[from] std::io::Error),

    /// Imagen no válida o no legible.
    #[error("Error de imagen: {0}")]
    Image(#[from] image::ImageError),

    /// Serialización / deserialización JSON.
    #[error("Error JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// Lectura del proyecto TOML.
    #[error("Error TOML: {0}")]
    TomlDe(#[from] toml::de::Error),

    /// Escritura del proyecto TOML.
    #[error("Error TOML: {0}")]
    TomlSer(#[from] toml::ser::Error),

    /// Renderizado de plantillas (handlebars).
    #[error("Error de plantilla: {0}")]
    Template(#[from] handlebars::RenderError),

    /// Primitivas de cifrado AES-GCM (el tipo de la crate no implementa
    /// `std::error::Error`, se convierte manualmente a texto).
    #[error("Error de cifrado: {0}")]
    Aead(String),

    /// Configuración del proyecto inválida.
    #[error("Configuración inválida: {0}")]
    Config(String),

    /// Conflicto de empaquetado (sprite que no cabe, hoja llena, etc.).
    #[error("{0}")]
    Pack(String),

    /// Otros errores con mensaje directo.
    #[error("{0}")]
    Other(String),
}

impl From<String> for TpError {
    fn from(message: String) -> Self {
        TpError::Other(message)
    }
}

impl From<&str> for TpError {
    fn from(message: &str) -> Self {
        TpError::Other(message.to_string())
    }
}

impl From<aes_gcm::Error> for TpError {
    fn from(err: aes_gcm::Error) -> Self {
        TpError::Aead(err.to_string())
    }
}

/// Resultado estándar de las operaciones del motor.
pub type Result<T> = std::result::Result<T, TpError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_preserves_message() {
        let err = TpError::Config("max_texture_size debe ser potencia de dos".into());
        assert_eq!(
            err.to_string(),
            "Configuración inválida: max_texture_size debe ser potencia de dos"
        );
    }

    #[test]
    fn from_string_and_str_map_to_other() {
        assert!(matches!(
            TpError::from("fallo".to_string()),
            TpError::Other(msg) if msg == "fallo"
        ));
        assert!(matches!(
            TpError::from("fallo"),
            TpError::Other(msg) if msg == "fallo"
        ));
    }

    #[test]
    fn aead_errors_convert_with_prefix() {
        let err: TpError = aes_gcm::Error.into();
        assert!(matches!(err, TpError::Aead(_)));
        assert!(err.to_string().starts_with("Error de cifrado:"));
    }

    #[test]
    fn io_errors_convert_with_prefix() {
        let err: TpError = std::fs::File::open("/definitivamente/inexistente")
            .unwrap_err()
            .into();
        assert!(matches!(err, TpError::Io(_)));
        assert!(err.to_string().starts_with("Error de E/S:"));
    }
}
