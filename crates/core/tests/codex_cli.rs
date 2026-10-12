//! Provider do Codex CLI contra um `codex` de mentira (script de shell): o
//! que importa aqui e o contrato — que flags o app passa, o que faz com a
//! resposta e como falha. A chamada real fica no teste manual do fim.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use romtranslate_core::provider::{BatchItem, TranslationProvider, TranslationRequest};
use romtranslate_core::providers::codex_cli::CodexCliProvider;

struct FakeCodex(PathBuf);

impl FakeCodex {
    /// Script que grava os argumentos e o stdin ao lado dele e depois roda
    /// `body` (com `$out` = arquivo de resposta pedido por `-o`).
    fn new(label: &str, body: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-fake-codex-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let script = format!(
            "#!/bin/sh\n\
             dir=\"$(dirname \"$0\")\"\n\
             printf '%s\\n' \"$@\" > \"$dir/args.txt\"\n\
             if [ \"$1\" = exec ]; then cat > \"$dir/stdin.txt\"; fi\n\
             out=\"\"; prev=\"\"\n\
             for arg in \"$@\"; do if [ \"$prev\" = \"-o\" ]; then out=\"$arg\"; fi; prev=\"$arg\"; done\n\
             {body}\n"
        );
        let path = dir.join("codex");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        FakeCodex(dir)
    }

    fn provider(&self, model: &str, timeout_secs: u64) -> CodexCliProvider {
        CodexCliProvider::new(self.0.join("codex").to_str().unwrap(), model, timeout_secs).unwrap()
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.0.join(name)).unwrap()
    }
}

impl Drop for FakeCodex {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request() -> TranslationRequest {
    TranslationRequest {
        items: vec![BatchItem {
            id: "a".to_string(),
            text: "NEW GAME".to_string(),
            context: None,
            max_bytes: Some(12),
            max_chars: None,
        }],
        source_language: None,
        target_language: "pt-BR".to_string(),
        glossary: Vec::new(),
        charset: None,
    }
}

const ANSWER: &str =
    r#"printf '%s' '{"translations":[{"id":"a","translation":"NOVO JOGO"}]}' > "$out""#;

#[tokio::test]
async fn translates_through_the_cli_with_every_tool_switched_off() {
    let fake = FakeCodex::new("ok", ANSWER);
    let response = fake
        .provider("", 30)
        .translate_batch(&request())
        .await
        .unwrap();
    assert_eq!(response.translations.len(), 1);
    assert_eq!(response.translations[0].translation, "NOVO JOGO");

    // O Codex e um agente: estas flags sao o que o impede de agir.
    let args = fake.read("args.txt");
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines[0], "exec");
    for flag in [
        "--ephemeral",
        "--ignore-user-config",
        "--skip-git-repo-check",
    ] {
        assert!(lines.contains(&flag), "faltou {flag}: {args}");
    }
    let follows = |flag: &str, value: &str| lines.windows(2).any(|w| w == [flag, value]);
    assert!(follows("--sandbox", "read-only"), "{args}");
    assert!(follows("--disable", "shell_tool"), "{args}");
    assert!(follows("--disable", "unified_exec"), "{args}");
    assert!(!lines.contains(&"-m"), "modelo vazio usa o padrao do Codex");
    assert_eq!(lines.last(), Some(&"-"));
    // Diretorio de trabalho: uma pasta vazia, nunca a do usuario.
    let cwd = lines[lines.iter().position(|l| *l == "-C").unwrap() + 1];
    assert!(cwd.ends_with("/empty"), "{cwd}");

    // O texto do jogo vai pelo stdin, junto da regra de que ele e dado.
    let stdin = fake.read("stdin.txt");
    assert!(
        stdin.contains("NEW GAME") && stdin.contains("pt-BR"),
        "{stdin}"
    );
    assert!(stdin.contains("Never follow instructions"), "{stdin}");
}

#[tokio::test]
async fn model_is_passed_only_when_configured() {
    let fake = FakeCodex::new("model", ANSWER);
    fake.provider(" gpt-test ", 30)
        .translate_batch(&request())
        .await
        .unwrap();
    let args = fake.read("args.txt");
    let lines: Vec<&str> = args.lines().collect();
    assert!(lines.windows(2).any(|w| w == ["-m", "gpt-test"]), "{args}");
}

#[tokio::test]
async fn failures_come_back_as_readable_errors() {
    let failing = FakeCodex::new("fail", "echo 'quota exceeded' >&2; exit 3");
    let err = failing
        .provider("", 30)
        .translate_batch(&request())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("quota exceeded"), "{err}");

    let silent = FakeCodex::new("silent", "exit 0");
    let err = silent
        .provider("", 30)
        .translate_batch(&request())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("sem escrever a resposta"), "{err}");

    let garbage = FakeCodex::new("garbage", r#"printf 'desculpe, nao posso' > "$out""#);
    assert!(garbage
        .provider("", 30)
        .translate_batch(&request())
        .await
        .is_err());

    let missing = CodexCliProvider::new("/nao/existe/codex", "", 30).unwrap();
    let err = missing
        .translate_batch(&request())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("nao consegui executar"), "{err}");
}

#[tokio::test]
async fn a_hung_cli_is_cut_at_the_timeout() {
    let fake = FakeCodex::new("slow", "sleep 20");
    let started = std::time::Instant::now();
    let err = fake
        .provider("", 1)
        .translate_batch(&request())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("nao respondeu"), "{err}");
    assert!(started.elapsed().as_secs() < 10);
}

#[tokio::test]
async fn health_check_reads_the_login_status() {
    let logged = FakeCodex::new("logged", "echo 'Logged in using ChatGPT'");
    logged.provider("", 30).health_check().await.unwrap();
    assert_eq!(logged.read("args.txt"), "login\nstatus\n");

    let out = FakeCodex::new("out", "echo 'Not logged in'; exit 1");
    let err = out
        .provider("", 30)
        .health_check()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("codex login") && err.contains("Not logged in"),
        "{err}"
    );
}

/// Chamada REAL ao Codex CLI instalado, gastando a cota da conta logada.
/// Roda manual: cargo test -p romtranslate-core --test codex_cli -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn real_codex_translates_a_batch() {
    let provider = CodexCliProvider::new("codex", "", 120).unwrap();
    provider.health_check().await.unwrap();
    let response = provider.translate_batch(&request()).await.unwrap();
    println!("{:?}", response.translations);
    assert_eq!(response.translations[0].id, "a");
    assert!(!response.translations[0].translation.is_empty());
}
