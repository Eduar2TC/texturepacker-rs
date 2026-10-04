//! Contratos de proceso: código de salida y stream por el que salen los
//! mensajes. No se pueden comprobar desde dentro de `main`, que termina con
//! `process::exit`, así que aquí se lanza el binario como lo haría un
//! script.

use std::process::Command;

/// Ejecuta `tp-cli` con los argumentos dados y devuelve (código, stdout,
/// stderr) ya decodificados.
fn tp_cli(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_tp-cli"))
        .args(args)
        .output()
        .expect("no se pudo lanzar el binario de tp-cli");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn sin_argumentos_es_un_error_de_uso() {
    // Un script que lance `tp-cli` sin nada no debe leerlo como un éxito:
    // la convención Unix para error de uso es 2 y el mensaje va a stderr.
    let (code, stdout, stderr) = tp_cli(&[]);
    assert_eq!(code, Some(2), "sin args debe salir con 2, no con 0");
    assert!(stdout.is_empty(), "la ayuda no debe ir a stdout: {stdout}");
    assert!(stderr.contains("tp-cli --help"), "{stderr}");
}

#[test]
fn decrypt_help_imprime_la_ayuda_de_decrypt() {
    let (code, stdout, stderr) = tp_cli(&["decrypt", "--help"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("decrypt <archivo.tpenc>"), "{stdout}");
    assert!(stdout.contains("--key"), "{stdout}");
    // Antes imprimía la ayuda de `pack`, con opciones que decrypt ni lee.
    assert!(!stdout.contains("--max-size"), "{stdout}");
    assert!(!stdout.contains("--auto-folders"), "{stdout}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn la_ayuda_general_sigue_en_stdout_con_exito() {
    for flag in ["--help", "-h"] {
        let (code, stdout, stderr) = tp_cli(&[flag]);
        assert_eq!(code, Some(0), "{flag}: {stderr}");
        assert!(stdout.contains("USOS:"), "{flag}: {stdout}");
        assert!(stdout.contains("--max-size"), "{flag}: {stdout}");
        assert!(stderr.is_empty(), "{flag}: {stderr}");
    }
}

#[test]
fn comando_desconocido_sigue_saliendo_con_1() {
    let (code, stdout, stderr) = tp_cli(&["nones"]);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("nones"), "{stderr}");
}
