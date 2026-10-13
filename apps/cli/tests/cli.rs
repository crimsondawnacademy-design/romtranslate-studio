//! A CLI de ponta a ponta, pelo binario de verdade: o ciclo inteiro numa
//! fixture sintetica, o scan por tabela com regiao, os erros e a traducao
//! contra um provider de mentira (nenhuma conta de verdade e chamada aqui).

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

use romtranslate_core::patch::apply_ips;
use romtranslate_core::relative::build_tbl;
use romtranslate_core::settings::{save_settings, AppSettings};
use romtranslate_core::synth::{self, CUSTOM_LINES, CUSTOM_LOWER, CUSTOM_UPPER};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-cli-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_string()
    }

    /// Grava `rom` como `name` e cria o projeto dele pela CLI. Devolve o
    /// caminho do projeto.
    fn project(&self, name: &str, rom: &[u8]) -> String {
        fs::write(self.0.join(name), rom).unwrap();
        let dir = self.path("game.rtsproj");
        ok(&["create", &self.path(name), &dir, "--source", "en-US"]);
        dir
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_romtranslate"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Roda e exige sucesso; devolve o que saiu no stdout.
fn ok(args: &[&str]) -> String {
    let output = run(args);
    assert!(output.status.success(), "{args:?}: {}", stderr(&output));
    String::from_utf8(output.stdout).unwrap()
}

/// Roda com `--json` e exige sucesso.
fn ok_json(args: &[&str]) -> Value {
    serde_json::from_str(&ok(&[args, &["--json"][..]].concat())).unwrap()
}

/// Roda e exige erro (saida 1); devolve o que saiu no stderr.
fn fails(args: &[&str]) -> String {
    let output = run(args);
    assert_eq!(output.status.code(), Some(1), "{args:?}");
    stderr(&output)
}

/// As frases do projeto, pelo `export`.
fn entries(tmp: &TempDir, project: &str) -> Vec<Value> {
    let file = tmp.path("entries.json");
    ok(&["export", project, &file]);
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

fn id_of(entries: &[Value], source: &str) -> String {
    let entry = entries
        .iter()
        .find(|e| e["sourceText"] == source)
        .unwrap_or_else(|| panic!("sem a frase {source}"));
    entry["id"].as_str().unwrap().to_string()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn full_cycle_on_a_synthetic_gba() {
    let tmp = TempDir::new("cycle");
    let rom = synth::make_gba_rom_with_pointers();
    let project = tmp.project("game.gba", &rom);

    let report = ok_json(&["inspect", &tmp.path("game.gba")]);
    assert_eq!(report["best"]["platform"], "gba");

    let extracted = ok_json(&["extract", &project]);
    assert_eq!(extracted["saved"], 7);

    // Uma traducao que cabe no lugar, uma maior com ponteiro em tabela (vai
    // pra outro lugar) e uma maior sem ponteiro (erro: nao tem pra onde ir).
    let all = entries(&tmp, &project);
    let tsv = tmp.path("translations.tsv");
    fs::write(
        &tsv,
        format!(
            "\u{feff}{}\tPOCAO\r\n\n{}\tNOVO JOGO AGORA\n{}\tFIM DE JOGO TOTAL\n",
            id_of(&all, "POTION"),
            id_of(&all, "NEW GAME"),
            id_of(&all, "GAME OVER"),
        ),
    )
    .unwrap();
    let applied = ok_json(&["apply", &project, &tsv]);
    assert_eq!(applied["applied"], 3);
    assert_eq!(applied["validation"]["errors"], 1, "{applied}");
    assert_eq!(applied["validation"]["byKind"]["error/byte_overflow"], 1);

    let validation = ok(&["validate", &project]);
    assert!(validation.contains("erros: 1"), "{validation}");
    assert!(
        validation.contains("error/byte_overflow: 1"),
        "{validation}"
    );
    assert!(
        validation.contains(&format!("erro em {}", id_of(&all, "GAME OVER"))),
        "{validation}"
    );

    // `block` recusa tudo enquanto houver erro; o padrao (`skip`) deixa a
    // frase com erro no original e grava o resto.
    let blocked = fails(&["reinsert", &project, "--policy", "block"]);
    assert!(blocked.contains("erro de validacao"), "{blocked}");
    let reinserted = ok_json(&["reinsert", &project]);
    assert_eq!(reinserted["apply"]["applied"], 2, "{reinserted}");
    assert_eq!(reinserted["apply"]["relocated"], 1);
    assert_eq!(reinserted["skippedErrors"], 1);
    assert_eq!(reinserted["verification"]["ok"], true);

    let working = fs::read(reinserted["workingPath"].as_str().unwrap()).unwrap();
    assert!(contains(&working, b"POCAO\0"));
    assert!(contains(&working, b"NOVO JOGO AGORA\0"));
    assert!(
        contains(&working, b"GAME OVER\0"),
        "a pulada fica no original"
    );
    assert!(!contains(&working, b"FIM DE JOGO TOTAL"));

    // O patch aplicado no original da a copia traduzida, byte a byte.
    let patch = ok_json(&["patch", &project, "--format", "ips"]);
    assert_eq!(patch["patchFormat"], "IPS");
    let ips = fs::read(patch["patchPath"].as_str().unwrap()).unwrap();
    assert_eq!(apply_ips(&rom, &ips).unwrap(), working);

    // O arquivo de origem nunca e tocado.
    assert_eq!(fs::read(tmp.0.join("game.gba")).unwrap(), rom);
}

#[test]
fn scan_reads_a_table_inside_a_region() {
    let tmp = TempDir::new("scan");
    let rom = synth::make_custom_table_rom();
    let project = tmp.project("game.nes", &rom);
    let tbl = tmp.path("game.tbl");
    fs::write(
        &tbl,
        build_tbl(&rom, Some(CUSTOM_UPPER), Some(CUSTOM_LOWER)).unwrap(),
    )
    .unwrap();

    // As frases ficam a partir de 0x100, uma a cada 0x40: a regiao pega a
    // segunda, a terceira e a quarta. `--tbl` sozinho ja escolhe `table`.
    let scan = ok_json(&["scan", &project, "--tbl", &tbl, "--region", "0x140..512"]);
    assert_eq!(scan["found"], 3, "{scan}");
    assert_eq!(scan["saved"], 3);
    assert_eq!(scan["scannedBytes"], 0xC0);
    let adopted = scan["tblPath"].as_str().unwrap();
    assert!(Path::new(adopted).starts_with(&project), "{adopted}");

    let found = entries(&tmp, &project);
    let texts: Vec<&str> = found
        .iter()
        .map(|e| e["sourceText"].as_str().unwrap())
        .collect();
    assert_eq!(texts, CUSTOM_LINES[1..4]);
    assert!(found.iter().all(|e| e["encoding"]["table"] == "game"));

    // Sem a regiao, o arquivo inteiro: as sete.
    let whole = ok_json(&["scan", &project, "--encoding", "table", "--tbl", &tbl]);
    assert_eq!(whole["found"], CUSTOM_LINES.len());

    // O ASCII nao le este cartucho, e o minimo de caracteres filtra.
    assert_eq!(ok_json(&["scan", &project])["found"], 0);
    let long = ok_json(&["scan", &project, "--tbl", &tbl, "--min-chars", "30"]);
    assert_eq!(long["found"], 3, "{long}");

    assert_eq!(fs::read(tmp.0.join("game.nes")).unwrap(), rom);
}

#[test]
fn bad_input_fails_with_a_clear_message() {
    let tmp = TempDir::new("errors");

    // Projeto que nao existe: erro, e nada e criado no lugar.
    let missing = tmp.path("missing.rtsproj");
    for command in [
        "extract", "scan", "estimate", "validate", "reinsert", "patch",
    ] {
        let message = fails(&[command, &missing]);
        assert!(
            message.contains("nao e um projeto do RomTranslate"),
            "{command}: {message}"
        );
    }
    assert!(fails(&["translate", &missing, "--config-dir", &tmp.path("cfg")]).contains("projeto"));
    // Pasta que existe e nao e projeto: nenhum banco nasce nela.
    let empty = tmp.path("empty");
    fs::create_dir(&empty).unwrap();
    fails(&["validate", &empty]);
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    assert!(!Path::new(&missing).exists());

    // Com `--json`, o erro tambem e JSON.
    let output = run(&["validate", &missing, "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_str(&stderr(&output)).unwrap();
    assert!(error["error"].as_str().unwrap().contains("missing.rtsproj"));

    // Arquivo que nao e jogo nenhum nao vira projeto.
    fs::write(tmp.0.join("noise.bin"), synth::make_random(4096, 7)).unwrap();
    let unknown = fails(&["create", &tmp.path("noise.bin"), &tmp.path("noise.rtsproj")]);
    assert!(unknown.contains("formato nao reconhecido"), "{unknown}");
    assert!(fails(&["inspect", &tmp.path("nope.gba")]).contains("nope.gba"));

    let project = tmp.project("game.gba", &synth::make_gba_rom_with_pointers());
    ok(&["extract", &project]);
    let potion = id_of(&entries(&tmp, &project), "POTION");

    // TSV malformado: a linha e o motivo, e NADA e gravado — nem a linha boa
    // que vem antes.
    let tsv = tmp.path("bad.tsv");
    fs::write(&tsv, format!("{potion}\tPOCAO\nlinha sem tab\n")).unwrap();
    let message = fails(&["apply", &project, &tsv]);
    assert!(
        message.contains("linha 2") && message.contains("id<TAB>traducao"),
        "{message}"
    );
    fs::write(&tsv, format!("{potion}\tPOCAO\nscan-ffffffff\tX\n")).unwrap();
    let message = fails(&["apply", &project, &tsv]);
    assert!(
        message.contains("linha 2") && message.contains("scan-ffffffff"),
        "{message}"
    );
    fs::write(&tsv, [0xFF, 0xFE, 0x00]).unwrap();
    assert!(fails(&["apply", &project, &tsv]).contains("bad.tsv"));
    assert!(fails(&["apply", &project, &tmp.path("nope.tsv")]).contains("nope.tsv"));
    assert!(entries(&tmp, &project)
        .iter()
        .all(|e| e["translatedText"].is_null()));

    // Sem nada reinserido nao ha do que fazer patch.
    fails(&["patch", &project]);
    // Regiao invertida e erro do core; tabela sem `table` e erro da CLI.
    assert!(fails(&["scan", &project, "--region", "0x200..0x100"]).contains("regiao invalida"));
    assert!(fails(&["scan", &project, "--tbl", &tsv, "--encoding", "ascii"]).contains("--tbl"));

    // Argumento invalido: saida 2, antes de qualquer trabalho.
    for args in [
        vec!["scan", &project, "--region", "abc"],
        vec!["scan", &project, "--encoding", "klingon"],
        vec!["reinsert", &project, "--policy", "maybe"],
        vec!["patch", &project, "--format", "zip"],
        vec!["frobnicate"],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
}

/// Um "Ollama" local de mentira: responde cada pedido com a traducao de
/// cada frase em minusculas (mesmo tamanho, mesmos placeholders).
fn fake_ollama() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim().to_ascii_lowercase();
                if line.is_empty() {
                    break;
                }
                if let Some(value) = line.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let items: Vec<Value> =
                serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
            let translations: Vec<Value> = items
                .iter()
                .map(|item| {
                    let text = item["text"].as_str().unwrap().to_lowercase();
                    json!({ "id": item["id"], "translation": text })
                })
                .collect();
            let content = json!({ "translations": translations }).to_string();
            let answer = json!({ "message": { "content": content } }).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
        }
    });
    url
}

/// Pasta de configuracao do app com o Ollama apontando pra `base_url`.
fn config_with_ollama(tmp: &TempDir, base_url: &str) -> String {
    let mut settings = AppSettings::default();
    settings.ollama.base_url = base_url.to_string();
    settings.ollama.model = "fake".to_string();
    settings.batch_size = 3;
    save_settings(&tmp.0.join("config"), &settings).unwrap();
    tmp.path("config")
}

#[test]
fn translate_uses_the_provider_from_the_app_settings() {
    let tmp = TempDir::new("translate");
    let project = tmp.project("game.gba", &synth::make_gba_rom_with_pointers());
    ok(&["extract", &project]);
    let config = config_with_ollama(&tmp, &fake_ollama());

    // Sem `--batch`, o tamanho do lote e o das configuracoes: 7 frases de 3 em 3.
    let estimate = ok_json(&["estimate", &project, "--config-dir", &config]);
    assert_eq!(estimate["entries"], 7);
    assert_eq!(estimate["batches"], 3);
    assert_eq!(
        ok_json(&["estimate", &project, "--batch", "50"])["batches"],
        1
    );

    let output = run(&["translate", &project, "--config-dir", &config, "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["translated"], 7, "{summary}");
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["stopped"], Value::Null);
    assert_eq!(summary["provider"], "ollama");
    assert_eq!(summary["model"], "fake");
    // O progresso vai pro stderr: o stdout e so o resultado.
    assert!(stderr(&output).contains("7 de 7"), "{}", stderr(&output));

    let all = entries(&tmp, &project);
    let potion = all.iter().find(|e| e["sourceText"] == "POTION").unwrap();
    assert_eq!(potion["translatedText"], "potion");
    assert_eq!(potion["status"], "machine");
    // A memoria global fica na pasta de configuracao que a CLI recebeu.
    assert!(tmp.0.join("config/global_tm.sqlite").exists());
    assert_eq!(
        ok_json(&["estimate", &project, "--batch", "50"])["entries"],
        0
    );
}

#[test]
fn translate_says_why_the_round_stopped() {
    let tmp = TempDir::new("stopped");
    let project = tmp.project("game.gba", &synth::make_gba_rom_with_pointers());
    ok(&["extract", &project]);
    // Porta sem ninguem ouvindo: o provider nao responde e a rodada para
    // (alguns segundos, pelas tentativas do core). Quantos lotes falham antes
    // de parar e conta do core — aqui so importa que nada foi traduzido.
    let free = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let config = config_with_ollama(&tmp, &format!("http://{free}"));

    let output = run(&["translate", &project, "--config-dir", &config]);
    assert_eq!(output.status.code(), Some(1));
    let (out, err) = (String::from_utf8_lossy(&output.stdout), stderr(&output));
    assert!(out.contains("traduzidas pelo provider: 0"), "{out}");
    assert!(!out.contains("falharam: 0"), "{out}");
    assert!(
        err.contains("a traducao parou") && err.contains("ollama inacessivel"),
        "{err}"
    );

    // Provider remoto sem a permissao das configuracoes: nem comeca.
    let settings = AppSettings {
        provider: "codex_cli".to_string(),
        ..AppSettings::default()
    };
    save_settings(&tmp.0.join("config"), &settings).unwrap();
    let refused = fails(&["translate", &project, "--config-dir", &config]);
    assert!(
        refused.contains("traducao remota desabilitada"),
        "{refused}"
    );
}
