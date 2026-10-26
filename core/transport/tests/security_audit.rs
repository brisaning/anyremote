//! Gate ciclo C8 (RNF-01/poda): **auditoría estática de secretos hardcodeados**
//! sobre todo el workspace `core/` + ausencia de verificadores TLS peligrosos
//! y de artefactos sensibles (`.pem`, `.key`, `.env`).
//!
//! Es una barrera fail-closed para futuros cambios: cualquier secreto introducido
//! en el código fuente hace fallar la suite.

use std::fs;
use std::path::{Path, PathBuf};

/// Palabras clave que, junto a un literal de texto ≥8 caracteres, indican secreto.
const SECRET_KEYWORDS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "private_key",
    "access_token",
    "refresh_token",
    "bearer ",
];

/// Extensiones prohibidas en el árbol fuente.
const FORBIDDEN_EXT: &[&str] = &["pem", "key", "pfx", "p12", "env"];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "target" || name == ".git" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Extrae literales de texto "..." del código fuente (simples, sin escapes complejos).
fn string_literals(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('"') {
            out.push(&after[..end]);
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

#[test]
fn no_hardcoded_secrets_in_workspace_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("core/");
    let mut files = Vec::new();
    walk(root, &mut files);
    assert!(files.len() > 20, "árbol fuente sospechosamente pequeño");

    let mut violations = Vec::new();

    for f in &files {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        if FORBIDDEN_EXT.contains(&ext) {
            violations.push(format!("artefacto prohibido: {}", f.display()));
            continue;
        }
        // Solo fuentes Rust/TOML.
        if !matches!(ext, "rs" | "toml" | "proto") {
            continue;
        }
        let Ok(text) = fs::read_to_string(f) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            // Este archivo documenta las políticas/patrones: no se autoescanea.
            if f.file_name().and_then(|s| s.to_str()) == Some("security_audit.rs") {
                break;
            }
            let low = line.to_lowercase();
            // Verificadores TLS peligrosos: prohibidos por política (RNF-01).
            if low.contains("dangerous") && low.contains("verifier") {
                violations.push(format!(
                    "{}:{}: verificador TLS peligroso",
                    f.display(),
                    n + 1
                ));
            }
            let has_kw = SECRET_KEYWORDS.iter().any(|k| low.contains(k));
            if !has_kw {
                continue;
            }
            // Este archivo documenta los patrones: no se autoescanea (co-ocurrencia deliberada).
            if f.file_name().and_then(|s| s.to_str()) == Some("security_audit.rs") {
                continue;
            }
            for lit in string_literals(line) {
                // El literal que es exactamente la keyword no es un secreto.
                if SECRET_KEYWORDS.contains(&lit) {
                    continue;
                }
                if lit.len() >= 8 && !lit.contains(' ') {
                    violations.push(format!(
                        "{}:{}: posible secreto literal {:?}",
                        f.display(),
                        n + 1,
                        lit
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "violaciones de política de secretos:\n{}",
        violations.join("\n")
    );
}

#[test]
fn tls_configuration_never_uses_dangerous_verifier() {
    // Regla de política explícita además del escaneo: el cliente de anyremote
    // valida contra root store (loopback: cert auto-firmado distribuido en banda).
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("core/");
    let lib = root.join("transport").join("src").join("lib.rs");
    let text = fs::read_to_string(&lib).expect("transport lib");
    assert!(
        !text.contains("dangerous"),
        "transport no debe contener material de verificador peligroso"
    );
    assert!(
        text.contains("with_root_certificates"),
        "el cliente debe anclar contra un root store"
    );
}
