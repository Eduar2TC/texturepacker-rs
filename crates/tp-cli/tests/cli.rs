//! Contratos de proceso: código de salida y stream por el que salen los
//! mensajes. No se pueden comprobar desde dentro de `main`, que termina con
//! `process::exit`, así que aquí se lanza el binario como lo haría un
//! script.

use std::process::Command;

/// Ejecuta `tp-cli` con los argumentos dados y devuelve (código, stdout,
/// stderr) ya decodificados. El locale va fijado de manera explícita: sin
/// él, el idioma de la salida dependería del equipo que lanza `cargo test`
/// y estos tests dejarían de decidir nada.
fn tp_cli(args: &[&str]) -> (Option<i32>, String, String) {
    tp_cli_in(args, "es_ES.UTF-8")
}

/// Igual que [`tp_cli`], pero con el locale dado (`LC_ALL` manda sobre
/// `LANG`, así que con una variable basta).
fn tp_cli_in(args: &[&str], locale: &str) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_tp-cli"))
        .args(args)
        .env("LC_ALL", locale)
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

/// El motivo por el que existe el i18n de la CLI: `LC_ALL=C` es el locale
/// que fija un script o un runner de CI, y su salida tiene que salir en
/// inglés para poder greppearla sin depender del equipo.
#[test]
fn con_locale_c_los_mensajes_salen_en_ingles() {
    let (code, _, stderr) = tp_cli_in(&[], "C.UTF-8");
    assert_eq!(code, Some(2), "{stderr}");
    assert!(
        stderr.contains("tp-cli: missing command"),
        "no salió en inglés: {stderr}"
    );

    let (code, _, stderr) = tp_cli_in(&["paquete"], "C.UTF-8");
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stderr.contains("error: Unknown command: paquete"),
        "no salió en inglés: {stderr}"
    );

    // Un error de uso de decrypt, que es donde un script mira.
    let (code, _, stderr) = tp_cli_in(&["decrypt", "a.tpenc"], "C.UTF-8");
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stderr.contains("decrypt needs --key PASSPHRASE"),
        "no salió en inglés: {stderr}"
    );
}

/// Y que el español sigue intacto: el proyecto es de origen hispano y la
/// traducción no puede costarle sus mensajes a quien habla español.
#[test]
fn con_locale_espanol_los_mensajes_se_quedan_en_espanol() {
    let (code, _, stderr) = tp_cli_in(&[], "es_MX.UTF-8");
    assert_eq!(code, Some(2), "{stderr}");
    assert!(
        stderr.contains("tp-cli: falta un comando"),
        "no salió en español: {stderr}"
    );

    let (code, _, stderr) = tp_cli_in(&["paquete"], "es_MX.UTF-8");
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stderr.contains("error: Comando desconocido: paquete"),
        "no salió en español: {stderr}"
    );

    let (code, _, stderr) = tp_cli_in(&["decrypt", "a.tpenc"], "es_MX.UTF-8");
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stderr.contains("decrypt necesita --key CLAVE"),
        "no salió en español: {stderr}"
    );
}
