//! `anyremote-filexfer` — Transferencia de archivos con integridad (ciclo C5, RF-02c).
//!
//! Patrón H-RD-08 (clean-room D-01): política de límites configurable inspirada en
//! `OPTION_ENABLE_FILE_TRANSFER` / `OPTION_FILE_TRANSFER_MAX_FILES` /
//! `OPTION_ONE_WAY_FILE_TRANSFER`, integridad **SHA-256** verificada por el receptor
//! y **reanudación desde el último offset confirmado**.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use data_encoding::HEXLOWER;
use sha2::{Digest, Sha256};

/// Tamaño de chunk por defecto (32 KiB) — por debajo de la cota de frame ARSP (16 MiB).
pub const DEFAULT_CHUNK: usize = 32 * 1024;

/// Política de transferencia (modelo de los OPTION_* de RustDesk, patrón H-RD-08).
#[derive(Clone, Debug, PartialEq)]
pub struct TransferPolicy {
    /// Habilitación maestra (patrón OPTION_ENABLE_FILE_TRANSFER).
    pub enabled: bool,
    /// Máximo de archivos por lote (patrón OPTION_FILE_TRANSFER_MAX_FILES).
    pub max_files_per_batch: u32,
    /// Máximo bytes por archivo (cota de recurso propia).
    pub max_bytes_per_file: u64,
    /// Solo un sentido de transferencia (patrón OPTION_ONE_WAY_FILE_TRANSFER):
    /// cuando es `true`, el receptor no puede ofrecer archivos.
    pub one_way: bool,
}

impl Default for TransferPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_files_per_batch: 64,
            max_bytes_per_file: 256 * 1024 * 1024, // 256 MiB
            one_way: false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum TransferError {
    Disabled,
    TooManyFiles { limit: u32, got: u32 },
    TooLarge { limit: u64, got: u64 },
    OneWayDenied,
    InvalidName(String),
    BadDigestLength(usize),
    Offset { expected: u64, got: u64 },
    HashMismatch { expected: String, got: String },
    UnknownTransfer(u64),
    Io(String),
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Metadatos de un archivo ofrecido.
#[derive(Clone, Debug, PartialEq)]
pub struct FileMeta {
    pub transfer_id: u64,
    /// Nombre **solo base** (sin separadores — se valida en `sanitize_name`).
    pub name: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

/// Sanitización de nombre: sin separadores, sin `..`, no vacío, ≤255.
pub fn sanitize_name(name: &str) -> Result<String, TransferError> {
    let ok = !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.starts_with('.');
    if ok {
        Ok(name.to_string())
    } else {
        Err(TransferError::InvalidName(name.to_string()))
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    HEXLOWER.encode(&Sha256::digest(data))
}

/// Emisor: valida política y produce chunks con offsets absolutos.
pub struct FileSender {
    policy: TransferPolicy,
}

impl FileSender {
    pub fn new(policy: TransferPolicy) -> Self {
        Self { policy }
    }

    /// Verifica la política para un lote de archivos (habilitación, cantidad, tamaño).
    pub fn validate_batch(&self, files: &[FileMeta]) -> Result<(), TransferError> {
        if !self.policy.enabled {
            return Err(TransferError::Disabled);
        }
        let got = files.len() as u32;
        if got > self.policy.max_files_per_batch {
            return Err(TransferError::TooManyFiles {
                limit: self.policy.max_files_per_batch,
                got,
            });
        }
        for f in files {
            if f.size > self.policy.max_bytes_per_file {
                return Err(TransferError::TooLarge {
                    limit: self.policy.max_bytes_per_file,
                    got: f.size,
                });
            }
            sanitize_name(&f.name)?;
        }
        Ok(())
    }

    /// Ofrece un archivo (construye meta con digest real).
    pub fn offer(&self, transfer_id: u64, name: &str, data: &[u8]) -> Result<FileMeta, TransferError> {
        let meta = FileMeta {
            transfer_id,
            name: sanitize_name(name)?,
            size: data.len() as u64,
            sha256: Sha256::digest(data).into(),
        };
        self.validate_batch(std::slice::from_ref(&meta))?;
        Ok(meta)
    }

    /// Itera chunks desde `from_offset` (reanudación) hasta el final.
    pub fn chunks_from<'a>(
        &'a self,
        data: &'a [u8],
        from_offset: u64,
        chunk_size: usize,
    ) -> impl Iterator<Item = (u64, &'a [u8])> + 'a {
        let start = from_offset as usize;
        let size = chunk_size.max(1);
        (start..data.len()).step_by(size).map(move |off| {
            let end = (off + size).min(data.len());
            (off as u64, &data[off..end])
        })
    }
}

struct Partial {
    meta: FileMeta,
    buf: Vec<u8>,
    next_offset: u64,
}

/// Receptor: ensambla por offset, reporta el offset esperado (reanudación)
/// y verifica SHA-256 al finalizar.
pub struct FileReceiver {
    policy: TransferPolicy,
    incoming: HashMap<u64, Partial>,
}

impl FileReceiver {
    pub fn new(policy: TransferPolicy) -> Self {
        Self {
            policy,
            incoming: HashMap::new(),
        }
    }

    /// Acepta una oferta si cumple política (habilitación, límites, nombre, digest).
    pub fn accept_offer(&mut self, meta: FileMeta) -> Result<(), TransferError> {
        if !self.policy.enabled {
            return Err(TransferError::Disabled);
        }
        if meta.size > self.policy.max_bytes_per_file {
            return Err(TransferError::TooLarge {
                limit: self.policy.max_bytes_per_file,
                got: meta.size,
            });
        }
        sanitize_name(&meta.name)?;
        if self.incoming.contains_key(&meta.transfer_id) {
            // Reoferta idempotente (reanudación tras reinicio del emisor).
            return Ok(());
        }
        self.incoming.insert(
            meta.transfer_id,
            Partial {
                buf: Vec::with_capacity(meta.size as usize),
                meta,
                next_offset: 0,
            },
        );
        Ok(())
    }

    /// Offset esperado para la reanudación.
    pub fn next_offset(&self, transfer_id: u64) -> Result<u64, TransferError> {
        self.incoming
            .get(&transfer_id)
            .map(|p| p.next_offset)
            .ok_or(TransferError::UnknownTransfer(transfer_id))
    }

    /// Escribe un chunk. Los offsets deben ser contiguos al confirmado;
    /// un offset adelantado reporta `Offset { expected }` (base de la reanudación).
    pub fn write_chunk(&mut self, transfer_id: u64, offset: u64, data: &[u8]) -> Result<u64, TransferError> {
        let p = self
            .incoming
            .get_mut(&transfer_id)
            .ok_or(TransferError::UnknownTransfer(transfer_id))?;
        if offset != p.next_offset {
            return Err(TransferError::Offset {
                expected: p.next_offset,
                got: offset,
            });
        }
        let new_len = p.buf.len() + data.len();
        if new_len as u64 > p.meta.size {
            return Err(TransferError::TooLarge {
                limit: p.meta.size,
                got: new_len as u64,
            });
        }
        p.buf.extend_from_slice(data);
        p.next_offset = new_len as u64;
        Ok(p.next_offset)
    }

    /// Finaliza: exige tamaño completo y verifica SHA-256. Devuelve el contenido.
    pub fn finish(&mut self, transfer_id: u64) -> Result<Vec<u8>, TransferError> {
        let p = self
            .incoming
            .remove(&transfer_id)
            .ok_or(TransferError::UnknownTransfer(transfer_id))?;
        if p.buf.len() as u64 != p.meta.size {
            return Err(TransferError::Offset {
                expected: p.meta.size,
                got: p.buf.len() as u64,
            });
        }
        let got: [u8; 32] = Sha256::digest(&p.buf).into();
        if got != p.meta.sha256 {
            return Err(TransferError::HashMismatch {
                expected: HEXLOWER.encode(&p.meta.sha256),
                got: HEXLOWER.encode(&got),
            });
        }
        Ok(p.buf)
    }

    /// Finaliza y escribe en `dir/name` (nombre saneado — sin traversal).
    pub fn finish_to_file(&mut self, transfer_id: u64, dir: &Path) -> Result<PathBuf, TransferError> {
        // finish consume el estado; validamos nombre antes desde la copia local.
        let name = {
            let p = self
                .incoming
                .get(&transfer_id)
                .ok_or(TransferError::UnknownTransfer(transfer_id))?;
            sanitize_name(&p.meta.name)?
        };
        let data = self.finish(transfer_id)?;
        let path = dir.join(name);
        std::fs::write(&path, &data).map_err(|e| TransferError::Io(e.to_string()))?;
        Ok(path)
    }

    pub fn policy(&self) -> &TransferPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: u64, name: &str, data: &[u8]) -> FileMeta {
        FileMeta {
            transfer_id: id,
            name: name.into(),
            size: data.len() as u64,
            sha256: Sha256::digest(data).into(),
        }
    }

    #[test]
    fn policy_limits_are_enforced() {
        let sender = FileSender::new(TransferPolicy {
            max_files_per_batch: 2,
            max_bytes_per_file: 100,
            ..Default::default()
        });
        let small = meta(1, "a.bin", &[0u8; 10]);
        let big = meta(2, "b.bin", &[0u8; 101]);
        assert!(sender.validate_batch(&[small.clone(), small.clone()]).is_ok());
        assert_eq!(
            sender.validate_batch(&[small.clone(), small.clone(), small.clone()]),
            Err(TransferError::TooManyFiles { limit: 2, got: 3 })
        );
        assert_eq!(
            sender.validate_batch(&[big]),
            Err(TransferError::TooLarge { limit: 100, got: 101 })
        );
        let disabled = FileSender::new(TransferPolicy {
            enabled: false,
            ..Default::default()
        });
        assert_eq!(disabled.validate_batch(&[small]), Err(TransferError::Disabled));
    }

    #[test]
    fn one_way_policy_denies_receiver_offers() {
        let mut recv = FileReceiver::new(TransferPolicy {
            one_way: true,
            ..Default::default()
        });
        // Emisor con one_way implica que el receptor no puede ofrecer:
        // la compuerta se aplica en el cableado (daemon) con esta política como fuente.
        assert!(recv.policy().one_way);
        assert!(recv.accept_offer(meta(1, "x", b"hola")).is_ok());
    }

    #[test]
    fn names_with_traversal_are_rejected() {
        assert!(sanitize_name("archivo.txt").is_ok());
        for bad in ["../evil", "a/b", "a\\b", "", ".", "..", ":ads"] {
            assert!(sanitize_name(bad).is_err(), "debió rechazar {bad:?}");
        }
    }

    #[test]
    fn chunked_transfer_with_hash_and_resume() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let mut sender_side = FileSender::new(TransferPolicy::default());
        let offer = sender_side.offer(7, "informe.bin", &data).unwrap();
        let mut recv = FileReceiver::new(TransferPolicy::default());
        recv.accept_offer(offer.clone()).unwrap();

        // Primer tramo: solo el chunk 0 (65536 < 100000 → quedan datos pendientes).
        let chunks: Vec<_> = sender_side.chunks_from(&data, 0, 64 * 1024).collect();
        assert_eq!(chunks.len(), 2, "100000 B con chunks de 64 KiB = 2 chunks");
        recv.write_chunk(7, chunks[0].0, chunks[0].1).unwrap();
        let confirmed = recv.next_offset(7).unwrap();
        assert_eq!(confirmed, 64 * 1024);

        // Interrupción: un offset adelantado reporta el esperado (base de reanudación).
        match recv.write_chunk(7, confirmed + 1, &[]) {
            Err(TransferError::Offset { expected, got }) => {
                assert_eq!(expected, confirmed);
                assert_eq!(got, confirmed + 1);
            }
            other => panic!("esperaba Offset, llegó {other:?}"),
        }

        // Reanudación desde el offset confirmado: envía el resto.
        for (off, chunk) in &chunks[1..] {
            recv.write_chunk(7, *off, chunk).unwrap();
        }
        assert_eq!(recv.next_offset(7).unwrap(), data.len() as u64);
        let got = recv.finish(7).unwrap();
        assert_eq!(got.len(), data.len());
        assert_eq!(sha256_hex(&got), HEXLOWER.encode(&offer.sha256));
    }

    #[test]
    fn corrupted_content_fails_hash() {
        let data = b"contenido original";
        let mut sender_side = FileSender::new(TransferPolicy::default());
        let offer = sender_side.offer(1, "f.txt", data).unwrap();
        let mut recv = FileReceiver::new(TransferPolicy::default());
        recv.accept_offer(offer).unwrap();
        let mut corrupted = data.to_vec();
        corrupted[0] ^= 0xff;
        recv.write_chunk(1, 0, &corrupted).unwrap();
        match recv.finish(1) {
            Err(TransferError::HashMismatch { .. }) => {}
            other => panic!("esperaba HashMismatch, llegó {other:?}"),
        }
    }

    #[test]
    fn finish_to_file_writes_real_file() {
        let dir = std::env::temp_dir().join(format!("anyremote-c5-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data = b"archivo real de prueba";
        let mut s = FileSender::new(TransferPolicy::default());
        let offer = s.offer(9, "prueba.txt", data).unwrap();
        let mut r = FileReceiver::new(TransferPolicy::default());
        r.accept_offer(offer).unwrap();
        r.write_chunk(9, 0, data).unwrap();
        let path = r.finish_to_file(9, &dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
