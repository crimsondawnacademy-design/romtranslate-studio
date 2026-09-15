//! Parser de cue sheet (.cue): resolve qual arquivo binario contem o track
//! de dados de um dump multi-track (PS1: track 1 MODE2/2352 + audio).
//! Formato CDRWIN/Redump: comandos FILE/TRACK/INDEX, tempos MM:SS:FF com 75
//! frames por segundo, relativos ao inicio de cada FILE.

use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

const MAX_CUE_SIZE: u64 = 1024 * 1024;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::Project(format!("cue: {}", msg.into()))
}

pub fn is_cue(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cue"))
}

#[derive(Debug, Clone)]
struct CueTrack {
    number: u32,
    mode: String,
    file: PathBuf,
    first_in_file: bool,
    index01_frames: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CueResolution {
    /// Arquivo que contem o track de dados (unico validado em disco —
    /// tracks de audio podem nem existir, nao sao tocados).
    pub bin_path: PathBuf,
    pub data_track: u32,
    pub data_mode: String,
    pub total_tracks: usize,
    pub audio_tracks: usize,
}

/// "MM:SS:FF" -> frames (75/s). None em formato invalido.
fn parse_msf(s: &str) -> Option<u32> {
    let mut parts = s.split(':');
    let m: u32 = parts.next()?.parse().ok()?;
    let sec: u32 = parts.next()?.parse().ok()?;
    let f: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || sec >= 60 || f >= 75 {
        return None;
    }
    Some(m.checked_mul(4500)? + sec * 75 + f)
}

/// Nome do comando FILE: entre aspas se houver, senao tudo antes do ultimo
/// token (o filetype, ex. BINARY) — cobre nome sem aspas com espacos.
fn file_name(rest: &str) -> &str {
    if let (Some(q1), Some(q2)) = (rest.find('"'), rest.rfind('"')) {
        if q2 > q1 {
            return &rest[q1 + 1..q2];
        }
    }
    rest.rsplit_once(char::is_whitespace)
        .map(|(name, _)| name.trim())
        .unwrap_or(rest)
}

fn parse(text: &str, base: &Path) -> Result<Vec<CueTrack>> {
    let mut tracks: Vec<CueTrack> = Vec::new();
    let mut current_file: Option<PathBuf> = None;
    let mut tracks_in_file = 0usize;

    for line in text.lines() {
        let line = line.trim();
        let Some(keyword) = line.split_whitespace().next() else {
            continue;
        };
        match keyword.to_ascii_uppercase().as_str() {
            "FILE" => {
                let name = file_name(line[4..].trim());
                if name.is_empty() {
                    return Err(err("comando FILE sem nome de arquivo"));
                }
                let path = Path::new(name);
                current_file = Some(if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    base.join(path)
                });
                tracks_in_file = 0;
            }
            "TRACK" => {
                let mut toks = line.split_whitespace().skip(1);
                let number: u32 = toks
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| err("TRACK sem numero"))?;
                let mode = toks
                    .next()
                    .ok_or_else(|| err(format!("TRACK {number} sem modo")))?
                    .to_ascii_uppercase();
                let file = current_file
                    .clone()
                    .ok_or_else(|| err(format!("TRACK {number} antes de qualquer FILE")))?;
                tracks_in_file += 1;
                tracks.push(CueTrack {
                    number,
                    mode,
                    file,
                    first_in_file: tracks_in_file == 1,
                    index01_frames: None,
                });
                if tracks.len() > 99 {
                    return Err(err("mais de 99 tracks"));
                }
            }
            "INDEX" => {
                let mut toks = line.split_whitespace().skip(1);
                let idx: Option<u32> = toks.next().and_then(|n| n.parse().ok());
                let msf = toks.next().and_then(parse_msf);
                if let (Some(1), Some(frames), Some(track)) = (idx, msf, tracks.last_mut()) {
                    track.index01_frames.get_or_insert(frames);
                }
            }
            _ => {} // REM, PREGAP, FLAGS, TITLE etc.
        }
    }
    if tracks.is_empty() {
        return Err(err("nenhum TRACK encontrado — isso e um cue sheet?"));
    }
    Ok(tracks)
}

/// Resolve um .cue pro arquivo do track de dados. Conservador: o track de
/// dados precisa ser o primeiro do seu FILE e comecar em INDEX 01 00:00:00
/// (layout Redump padrao); qualquer outro layout da erro claro em vez de
/// ler dados da posicao errada.
pub fn resolve_cue(cue_path: &Path) -> Result<CueResolution> {
    let meta = std::fs::metadata(cue_path).map_err(|e| CoreError::io(cue_path, e))?;
    if meta.len() > MAX_CUE_SIZE {
        return Err(err(
            "arquivo .cue grande demais (>1 MiB) — isso e um cue sheet?",
        ));
    }
    let bytes = std::fs::read(cue_path).map_err(|e| CoreError::io(cue_path, e))?;
    let text = String::from_utf8_lossy(&bytes); // cues antigos vem em windows-1252
    let base = cue_path.parent().unwrap_or(Path::new("."));
    let tracks = parse(&text, base)?;

    let data = tracks
        .iter()
        .find(|t| t.mode.starts_with("MODE"))
        .ok_or_else(|| err("nenhum track de dados (MODE1/MODE2) — cue so de audio?"))?;
    if !data.first_in_file {
        return Err(err(format!(
            "track de dados {} nao e o primeiro do seu FILE — layout nao suportado",
            data.number
        )));
    }
    if data.index01_frames != Some(0) {
        return Err(err(format!(
            "track de dados {} nao comeca em INDEX 01 00:00:00 — layout nao suportado",
            data.number
        )));
    }
    if !data.file.is_file() {
        return Err(err(format!(
            "arquivo do track de dados nao encontrado: {}",
            data.file.display()
        )));
    }
    Ok(CueResolution {
        bin_path: data.file.clone(),
        data_track: data.number,
        data_mode: data.mode.clone(),
        total_tracks: tracks.len(),
        audio_tracks: tracks.iter().filter(|t| t.mode == "AUDIO").count(),
    })
}

/// Entrada unica pros fluxos que recebem path de ROM: .cue resolve pro BIN
/// do track de dados; qualquer outro arquivo passa direto.
pub fn resolve_source(path: &Path) -> Result<(PathBuf, Option<CueResolution>)> {
    if is_cue(path) {
        let r = resolve_cue(path)?;
        Ok((r.bin_path.clone(), Some(r)))
    } else {
        Ok((path.to_path_buf(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_unquoted_lowercase_and_msf() {
        let base = Path::new("/discs");
        let tracks = parse(
            "REM ripado\r\nfile \"My Game (BR).bin\" binary\r\n  track 01 mode2/2352\r\n    index 01 00:00:00\r\n  TRACK 02 AUDIO\r\n    INDEX 00 05:58:00\r\n    INDEX 01 06:00:74\r\nFILE bonus.bin BINARY\r\n  TRACK 03 AUDIO\r\n    INDEX 01 00:00:00\r\n",
            base,
        )
        .unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].file, base.join("My Game (BR).bin"));
        assert_eq!(tracks[0].mode, "MODE2/2352");
        assert!(tracks[0].first_in_file);
        assert_eq!(tracks[0].index01_frames, Some(0));
        assert_eq!(tracks[1].index01_frames, Some(6 * 60 * 75 + 74));
        assert!(!tracks[1].first_in_file);
        assert_eq!(tracks[2].file, base.join("bonus.bin"));
        assert!(tracks[2].first_in_file);
    }

    #[test]
    fn garbage_and_edge_inputs_error_cleanly() {
        let base = Path::new(".");
        assert!(parse("", base).is_err());
        assert!(parse("nao e um cue\n\0\x01\x02", base).is_err());
        assert!(parse("TRACK 01 MODE2/2352\n", base).is_err()); // TRACK antes de FILE
        assert!(parse("FILE \"a.bin\" BINARY\nTRACK xx MODE2/2352\n", base).is_err());
        // INDEX invalido nao derruba o parse (so nao registra):
        let t = parse(
            "FILE \"a.bin\" BINARY\nTRACK 01 MODE2/2352\nINDEX 01 99:99:99\n",
            base,
        )
        .unwrap();
        assert_eq!(t[0].index01_frames, None);
    }
}
