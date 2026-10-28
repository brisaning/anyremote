//! CLI `anyremoted id` (C-9, D-03): devuelve el ID estable del equipo en una
//! línea (Base32 de 10 caracteres), cargando el MISMO `identity.key` que usa
//! `listen`. Estable entre invocaciones (identidad persistente v0.8.0).

use std::path::{Path, PathBuf};

fn temp_cfg(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anyremote-cliid-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

fn run_id(bin: &str, cfg: &Path) -> (bool, String, String) {
    let out = std::process::Command::new(bin)
        .args(["id", "--config-dir", cfg.to_str().expect("utf8")])
        .output()
        .expect("spawn anyremoted id");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn id_subcommand_prints_a_stable_well_formed_node_id() {
    let cfg = temp_cfg("id");
    let bin = env!("CARGO_BIN_EXE_anyremoted");

    let (ok, id, err) = run_id(bin, &cfg);
    assert!(ok, "exit 0 — stderr: {err}");
    assert_eq!(id.len(), 10, "ID de 10 caracteres: {id}");
    assert!(
        id.chars().all(|c| matches!(c, 'A'..='Z' | '2'..='7')),
        "Base32 Crockford (A–Z, 2–7): {id}"
    );
    assert!(cfg.join("identity.key").exists(), "persiste identity.key");

    // Identidad estable: otra invocación → el MISMO ID (D-03 / v0.8.0).
    let (ok2, id2, err2) = run_id(bin, &cfg);
    assert!(ok2, "exit 0 (2ª llamada) — stderr: {err2}");
    assert_eq!(id, id2, "el ID es estable entre invocaciones");

    let _ = std::fs::remove_dir_all(&cfg);
}
