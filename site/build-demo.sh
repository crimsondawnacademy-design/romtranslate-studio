#!/usr/bin/env bash
# Compila a interface de verdade do app, com o backend simulado de
# apps/desktop/.preview (jogo inventado, nada sai da pagina), para site/demo/.
# A saida fica fora do git. Antes: pnpm install --frozen-lockfile na raiz.
set -euo pipefail

SITE="$(cd "$(dirname "$0")" && pwd)"
cd "$SITE/../apps/desktop"

# A pagina de entrada e .preview/index.html. A raiz do Vite vai para .preview
# para o index.html sair na raiz de site/demo/; o alias devolve /src/ ao app.
node --input-type=module -e '
import { build } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
await build({
  configFile: false,
  root: resolve(".preview"),
  base: "./",
  plugins: [react()],
  resolve: { alias: [{ find: /^\/src\//, replacement: resolve("src") + "/" }] },
  build: { outDir: process.argv[1], emptyOutDir: true },
});
' "$SITE/demo"

# O simulador e um <script> comum (sem type=module): o Vite nao o empacota.
cp .preview/mock-tauri.js "$SITE/demo/"
test -s "$SITE/demo/index.html" && test -s "$SITE/demo/mock-tauri.js"

# O logo do site e o do app: a copia e refeita aqui, e o git mostra se mudou.
cp src/assets/logo.svg "$SITE/logo.svg"
echo "demo pronta em $SITE/demo"
