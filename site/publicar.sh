#!/usr/bin/env bash
# Publica o site no Cloudflare Pages. Quem roda e o dono do projeto, a mao.
# Compila a demonstracao, confere os links e so entao sobe a pasta site/.
set -euo pipefail

SITE="$(cd "$(dirname "$0")" && pwd)"
"$SITE/build-demo.sh"
node "$SITE/conferir.mjs"
cd "$SITE/.."
wrangler pages deploy site --project-name romtranslate-studio --branch main
