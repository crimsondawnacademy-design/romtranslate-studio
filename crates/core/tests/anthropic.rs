//! Chamadas REAIS a API da Anthropic, com a chave do ambiente. Ficam fora do
//! CI. O formato do pedido e da resposta tem teste sem rede em
//! `providers/anthropic.rs`.
//!
//! ANTHROPIC_API_KEY=... cargo test -p romtranslate-core --test anthropic -- --ignored --nocapture

use romtranslate_core::provider::{BatchItem, TranslationProvider, TranslationRequest};
use romtranslate_core::providers::anthropic::AnthropicProvider;

fn provider() -> AnthropicProvider {
    let key = std::env::var("ANTHROPIC_API_KEY").expect("defina ANTHROPIC_API_KEY");
    let model = std::env::var("ANTHROPIC_MODEL").unwrap_or_default();
    AnthropicProvider::new("", &model, Some(key), 120).unwrap()
}

fn request() -> TranslationRequest {
    TranslationRequest {
        items: vec![BatchItem {
            id: "a".to_string(),
            text: "NEW GAME".to_string(),
            context: None,
            max_bytes: Some(12),
            max_chars: None,
            max_line: None,
        }],
        source_language: None,
        target_language: "pt-BR".to_string(),
        glossary: Vec::new(),
        charset: None,
    }
}

/// Nao cobra: confere a chave e o modelo e conta os tokens de um lote.
#[tokio::test]
#[ignore]
async fn real_api_accepts_the_key_and_counts_a_batch() {
    let provider = provider();
    provider.health_check().await.unwrap();
    let tokens = provider.count_input_tokens(&request()).await.unwrap();
    println!("tokens de entrada do lote: {tokens}");
    assert!(tokens > 20);
}

/// COBRA alguns tokens: traduz um lote de uma frase.
#[tokio::test]
#[ignore]
async fn real_api_translates_a_batch_and_reports_what_it_billed() {
    let provider = provider();
    let response = provider.translate_batch(&request()).await.unwrap();
    println!(
        "{:?} | cobrado: {:?}",
        response.translations,
        provider.usage()
    );
    assert_eq!(response.translations[0].id, "a");
    assert!(provider.usage().input_tokens > 0 && provider.usage().output_tokens > 0);
}
