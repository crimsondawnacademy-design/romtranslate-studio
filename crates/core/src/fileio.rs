//! Leitura de imagens grandes por memory-map: o SO pagina sob demanda e a
//! RAM usada fica no page cache — e o que destrava DVD de PS2 (4.7-8.5 GiB)
//! sem carregar o arquivo inteiro. Premissa (a mesma da leitura normal):
//! ninguem modifica o arquivo durante a operacao — o original do usuario e
//! sagrado e a working copy e nossa.

use std::fs::File;
use std::ops::Deref;
use std::path::Path;

use crate::adapter::MAX_FILE_SIZE;
use crate::error::{CoreError, Result};

pub enum FileBytes {
    Owned(Vec<u8>),
    Mapped(memmap2::Mmap),
}

impl Deref for FileBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            FileBytes::Owned(v) => v,
            FileBytes::Mapped(m) => m,
        }
    }
}

/// Abre o arquivo como `&[u8]` sem carregar tudo em RAM (mmap read-only).
/// Cap: MAX_FILE_SIZE (16 GiB) — o teto de inspecao, nao mais o de memoria.
pub fn read_view(path: &Path) -> Result<FileBytes> {
    let file = File::open(path).map_err(|e| CoreError::io(path, e))?;
    let len = file.metadata().map_err(|e| CoreError::io(path, e))?.len();
    if len > MAX_FILE_SIZE {
        return Err(CoreError::FileTooLarge {
            size: len,
            limit: MAX_FILE_SIZE,
        });
    }
    if len == 0 {
        return Ok(FileBytes::Owned(Vec::new())); // mmap de arquivo vazio nao e portavel
    }
    // SAFETY: mapa somente leitura de arquivo regular; assumimos que o
    // arquivo nao e truncado/alterado durante a operacao (premissa acima).
    let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| CoreError::io(path, e))?;
    Ok(FileBytes::Mapped(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_matches_read_and_empty_is_ok() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!(
            "rts-fileio-{}.bin",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&p, b"conteudo qualquer").unwrap();
        assert_eq!(&read_view(&p).unwrap()[..], b"conteudo qualquer");
        std::fs::write(&p, b"").unwrap();
        assert_eq!(read_view(&p).unwrap().len(), 0);
        let _ = std::fs::remove_file(&p);
    }
}
