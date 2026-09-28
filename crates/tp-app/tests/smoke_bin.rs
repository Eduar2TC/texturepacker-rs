//! Autotest de la app completa: compila y ejecuta el binario `tp-smoke`,
//! que genera un proyecto temporal, lo abre con la app real headless y
//! verifica la vista previa sin interacción humana. Si este test pasa, la
//! app abre proyectos y muestra atlas de verdad (no solo piezas sueltas).

use std::process::Command;

#[test]
fn tp_smoke_binary_runs() {
    let bin = env!("CARGO_BIN_EXE_tp-smoke");
    let out = Command::new(bin)
        .arg("--json")
        .env("TP_LOG_STDERR", "1")
        .output()
        .expect("ejecutar tp-smoke");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "tp-smoke falló ({})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    assert!(
        stdout.contains("\"ok\":true"),
        "el resumen JSON debería confirmar el paso: {stdout}"
    );
}
