// Conferencia do site sem navegador: node site/conferir.mjs
// Falha (saida 1) se um link de download nao tiver o nome exato do arquivo da
// versao, se uma ancora ou arquivo local nao existir, ou se as duas paginas
// (pt-BR e en) sairem do mesmo molde. O publicar.sh roda isto antes do deploy.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SITE = dirname(fileURLToPath(import.meta.url));
const v = readFileSync(join(SITE, "versao.js"), "utf8").match(/VERSAO = "([^"]+)"/)[1];
// Os nomes que o release do GitHub publica (tauri-action), nesta ordem na pagina.
const ARQUIVOS = [
  `RomTranslate.Studio_${v}_aarch64.dmg`,
  `RomTranslate.Studio_${v}_x64.dmg`,
  `RomTranslate.Studio_${v}_x64-setup.exe`,
  `RomTranslate.Studio_${v}_x64_en-US.msi`,
  `RomTranslate.Studio_${v}_amd64.deb`,
  `RomTranslate.Studio-${v}-1.x86_64.rpm`,
  `RomTranslate.Studio_${v}_amd64.AppImage`,
];

const erros = [];
const todos = (html, re) => [...html.matchAll(re)].map((m) => m[1]);
const molde = {};
for (const pagina of ["index.html", "en/index.html"]) {
  const html = readFileSync(join(SITE, pagina), "utf8");
  const pasta = dirname(join(SITE, pagina));

  const baixar = todos(html, /data-arquivo="([^"]+)"/g).map((t) => t.replaceAll("{v}", v));
  if (baixar.join() !== ARQUIVOS.join()) erros.push(`${pagina}: downloads diferentes do esperado: ${baixar.join(", ")}`);

  const ids = new Set(todos(html, /\sid="([^"]+)"/g));
  for (const alvo of todos(html, /href="#([^"]+)"/g))
    if (!ids.has(alvo)) erros.push(`${pagina}: ancora #${alvo} sem destino`);
  for (const alvo of todos(html, /data-alvo="#([^"]+)"/g))
    for (const os of ["macos", "windows", "linux"])
      if (!ids.has(alvo.replace("{os}", os))) erros.push(`${pagina}: bloco #${alvo.replace("{os}", os)} nao existe`);

  for (const ref of todos(html, /(?:href|src)="([^"#]+)"/g)) {
    if (/^(https?:|mailto:)/.test(ref)) continue;
    const arquivo = join(pasta, ref.split("?")[0]);
    const alvo = ref.endsWith("/") ? join(arquivo, "index.html") : arquivo;
    if (!existsSync(alvo))
      erros.push(`${pagina}: ${ref} nao existe${ref.includes("demo/") ? " (rode site/build-demo.sh)" : ""}`);
  }

  // As duas paginas tem o mesmo conteudo: mesmas contagens de cada peca.
  for (const peca of ["<section", "<details", "<tr>", 'class="card', "<h2", "<h3", "<h4", "data-count="])
    (molde[peca] ??= []).push(html.split(peca).length - 1);
}
for (const [peca, [pt, en]] of Object.entries(molde))
  if (pt !== en) erros.push(`paginas fora do molde: ${peca} aparece ${pt} vezes em pt-BR e ${en} em en`);

if (erros.length) {
  console.error(erros.join("\n"));
  process.exit(1);
}
console.log(`site conferido: versao ${v}, ${ARQUIVOS.length} downloads, duas paginas no mesmo molde`);
