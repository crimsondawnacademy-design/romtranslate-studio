// Simulador do backend Tauri para conferir a interface no navegador, sem Rust.
// Ferramenta de desenvolvimento: nao entra no app compilado. Tudo aqui e ficticio: jogo, falas, caminhos.
//
// Uso:
//   pnpm --dir apps/desktop exec vite --port 1437
//   abrir http://localhost:1437/.preview/index.html
//
// Parametros da URL (todos opcionais):
//   auto=1        abre o projeto, valida, seleciona uma fala, traduz e reinsere sozinho.
//                 Outros valores param antes: home, drop (com tauri=1), inspecting
//                 (com slow=60000), report, created (com fresh=1), project, tools
//                 (busca relativa + Raio-X + glossario abertos).
//                 Ao chegar, a pagina marca <html data-ready="1">.
//   lang=en-US    forca o idioma da interface (padrao: o do navegador)
//   entries=0     projeto sem nenhum texto extraido (estado vazio)
//   res=1         mostra a coluna "Arquivo" da tabela
//   provider=anthropic | openai_compatible | codex_cli   (padrao: ollama)
//   nomatch=1     a deteccao nao reconhece o arquivo
//   support=experimental | partial | extract_only | unsupported
//   missing=1 / changed=1   projeto com a origem sumida / alterada
//   testfail=1    "Testar conexao" falha
//   slow=4000     a deteccao demora (ms), para ver a tela de espera
//   tick=250      intervalo do progresso da traducao (ms)
//   tauri=1       liga o arrastar-e-soltar; no console:
//                 __mock.emit("tauri://drag-enter", { paths: ["/x.rtsf"], position: { x: 0, y: 0 } })
(() => {
  const q = new URLSearchParams(location.search);
  if (q.get("lang")) Object.defineProperty(navigator, "language", { value: q.get("lang") });

  const SRC = "/Users/demo/Jogos/brisa-e-o-farol-de-papel.rtsf";
  const DIR = "/Users/demo/Jogos/brisa-e-o-farol-de-papel.rtsproj";
  const hex = (s) =>
    [...new TextEncoder().encode(s)].map((b) => b.toString(16).padStart(2, "0")).join("");

  // [original, traducao, status]
  const lines = [
    ["Welcome to Paper Lighthouse, {name}!", "Boas-vindas ao Farol de Papel, {name}!", "reviewed"],
    ["Press START to light the lamp.", "Aperte START para acender a lâmpada.", "reviewed"],
    ["The tide left a letter on the stairs.", "A maré deixou uma carta na escada.", "machine"],
    ["You found 3 paper boats.", "Você achou 3 barquinhos de papel.", "machine"],
    ["Tico: I folded too many cranes again.", "Tico: Dobrei grous demais outra vez, e agora eles não cabem na gaveta.", "machine"],
    ["Save your progress?", "Salvar o progresso?", "reviewed"],
    ["The door needs {n} keys.", "A porta precisa de chaves.", "machine"],
    ["Lamp oil: {n}/{max}", "Óleo da lâmpada: {n}/{max}", "reviewed"],
    ["It is too dark to read the sign.", "Está escuro demais para ler a placa.", "machine"],
    ["The map is torn in half.", null, "error"],
    ["Old Maré hums by the window.", null, "untranslated"],
    ["ROOM_LIGHT_03", null, "internal"],
    ["Thank you, little spark.", "Obrigada, faisquinha.", "machine"],
    ["The bell rings three times.", "O sino toca três vezes.", "machine"],
    ["Pip joined your crew!", "Pip entrou na sua tripulação!", "reviewed"],
    ["A sleepy gull blocks the way.", null, "untranslated"],
    ["SCR_INTRO_A", null, "internal"],
    ["Nothing happens.", "Nada acontece.", "machine"],
    ["The kite string snapped.", null, "error"],
    ["The lighthouse sleeps until dawn.", null, "untranslated"],
  ];
  const entries = (q.get("entries") === "0" ? [] : lines).map(([src, tr, status], i) => ({
    id: `e${i}`,
    resourcePath: q.get("res") ? `text/area_${String(1 + (i % 4)).padStart(2, "0")}.msg` : null,
    offset: 0x1a40 + i * 0x2c,
    originalBytes: hex(src),
    sourceText: src,
    translatedText: tr,
    context: null,
    maxBytes: null,
    encoding: "utf8",
    status,
    metadata: i % 6 === 0 ? { pointers: [0x200 + i * 4] } : null,
  }));
  const issues = entries.length
    ? [
        { entryId: "e4", severity: "error", kind: "byte_overflow", message: "A tradução tem 72 bytes e o espaço da original é de 37 bytes. Encurte o texto." },
        { entryId: "e4", severity: "warning", kind: "length_anomaly", message: "A tradução é bem mais longa que o original." },
        { entryId: "e6", severity: "error", kind: "placeholder_mismatch", message: "Falta o placeholder {n} na tradução." },
        { entryId: "e3", severity: "warning", kind: "length_anomaly", message: "A tradução é bem mais longa que o original." },
        { entryId: "e12", severity: "warning", kind: "length_anomaly", message: "A tradução é mais curta que o esperado para esta fala." },
      ]
    : [];

  let settings = {
    provider: q.get("provider") || "ollama",
    ollama: { baseUrl: "http://localhost:11434", model: "llama3.2:3b" },
    openaiCompatible: { baseUrl: "https://api.example.test/v1", model: "" },
    anthropic: { baseUrl: "", model: "claude-opus-5-5" },
    codexCli: { command: "codex", model: "", imageModel: "", ownAccount: false },
    priceInput: 0,
    priceOutput: 0,
    allowRemoteTranslation: false,
    batchSize: 20,
    timeoutSecs: 120,
  };
  const project = {
    id: "demo",
    sourcePath: SRC,
    sourceSha256: "9f2c41d07ab85e6630c1f7d2a94be10577c3e8d4f1a6b09c2d5e7f8091a3b4c6",
    sourceSize: 4194304,
    platform: "synthetic",
    adapterId: "rtsf-synthetic",
    sourceLanguage: "en-US",
    targetLanguage: "pt-BR",
    status: "created",
    createdAt: "2026-10-13T12:00:00Z",
  };
  const best = {
    adapterId: "rtsf-synthetic",
    platform: "synthetic",
    confidence: 0.97,
    evidence: [
      "magic RTSF no offset 0",
      "tabela de strings com 20 entradas",
      "checksum do cabeçalho confere",
    ],
    supportLevel: q.get("support") || "full",
  };
  const report = {
    path: SRC,
    size: project.sourceSize,
    sha256: project.sourceSha256,
    results: q.get("nomatch")
      ? []
      : [best, { ...best, adapterId: "generic-scan", platform: "unknown", confidence: 0.2, supportLevel: "extract_only" }],
    best: q.get("nomatch") ? null : best,
  };

  const callbacks = new Map();
  const listeners = new Map();
  let nextId = 1;
  const emit = (event, payload) => {
    for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
  };
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const count = (status) => entries.filter((e) => e.status === status).length;

  const handlers = {
    "plugin:event|listen": ({ event, handler }) => {
      listeners.set(event, [...(listeners.get(event) ?? []), handler]);
      return handler;
    },
    "plugin:event|unlisten": () => null,
    "plugin:dialog|open": ({ options }) => (options?.directory ? DIR : SRC),
    "plugin:dialog|save": ({ options }) => options?.defaultPath ?? null,
    "plugin:opener|reveal_item_in_dir": () => null,
    inspect_file: async () => (await sleep(Number(q.get("slow") ?? 300)), report),
    open_project: () => {
      if (q.get("fresh")) throw "projeto não encontrado";
      return { project, sourceFound: !q.get("missing"), sourceChanged: !!q.get("changed") };
    },
    create_project: () => project,
    load_entries: () => entries,
    save_entries: () => entries.length,
    glossary_list: () =>
      entries.length
        ? [
            { term: "Paper Lighthouse", translation: "Farol de Papel", noTranslate: false, caseSensitive: false, note: null },
            { term: "START", translation: null, noTranslate: true, caseSensitive: false, note: null },
          ]
        : [],
    glossary_upsert: () => null,
    glossary_delete: () => null,
    get_settings: () => ({ settings, apiKeySet: false, anthropicKeySet: true }),
    save_settings: ({ newSettings }) => ((settings = newSettings), { settings, apiKeySet: false, anthropicKeySet: true }),
    scan_project: () => ({ entries, truncated: false, scannedBytes: project.sourceSize, discarded: 212, tblPath: null }),
    extract_structured: () => ({ saved: entries.length, discarded: 18, truncated: false, removed: 0, stray: 0, packed: 0, jis16: 0, banked: 0 }),
    xray_project: () => ({
      size: project.sourceSize,
      denseBytes: 2600000,
      blankBytes: 500000,
      lz77: { blocks: 14, packedBytes: 182000, unpackedBytes: 410000, blocksWithText: 3, strings: 120, truncated: false, first: [] },
      entries: entries.length,
      relocatable: Math.ceil(entries.length / 6),
    }),
    relative_search: () => [
      { upperBase: 0x80, lowerBase: null, hits: 12, offsets: [0x1a40], preview: "START ..ELCOME TO PAPER ..GHTHOUSE", ascii: false },
      { upperBase: 0x41, lowerBase: null, hits: 3, offsets: [0x2000], preview: "START", ascii: true },
    ],
    save_relative_tbl: () => `${DIR}/tables/relative.tbl`,
    validate_project: () => ({
      issues,
      errors: issues.filter((i) => i.severity === "error").length,
      warnings: issues.filter((i) => i.severity === "warning").length,
      checked: entries.filter((e) => e.translatedText).length,
    }),
    estimate_translation: () => ({ entries: count("untranslated"), batches: 1, inputTokens: 1840, outputTokens: 420, calibrated: false }),
    test_provider: () => {
      if (q.get("testfail")) throw "conexão recusada em http://localhost:11434";
      return "llama3.2:3b";
    },
    translate_project: async () => {
      const total = entries.length;
      for (let done = 0; done <= total; done += 2) {
        emit("translation-progress", { done, total, phase: done < 4 ? "tm" : "translate" });
        await sleep(Number(q.get("tick") ?? 250));
      }
      return { translated: count("machine"), tmHits: 3, failed: count("error"), cancelled: false, stopped: null, shortened: 0, tooLong: 0, notText: 0, inputTokens: 0, outputTokens: 0 };
    },
    cancel_translation: () => null,
    update_entry: ({ id }) => issues.filter((i) => i.entryId === id),
    set_entry_reviewed: () => "reviewed",
    unmark_internal: () => null,
    // Texto em imagem: tres imagens ficticias com "letras" desenhadas na hora.
    list_pictures: () => [
      { id: "img-01ba0000", offset: 0x1ba0000, packed: null, bytes: 49152, lettered: 315 },
      { id: "img-01c8deb4", offset: 0x1c8deb4, packed: 3441, bytes: 17408, lettered: 248 },
      { id: "img-00fe53e4", offset: 0xfe53e4, packed: 2210, bytes: 8192, lettered: 96 },
    ],
    picture_png: async ({ id }) => {
      const canvas = document.createElement("canvas");
      canvas.width = 256;
      canvas.height = 128;
      const g = canvas.getContext("2d");
      g.fillStyle = "#eee";
      g.fillRect(0, 0, 256, 128);
      g.fillStyle = "#222";
      g.font = "14px sans-serif";
      g.fillText(id.endsWith("deb4") ? "警告－健康と安全のために" : "おまかせ  もどる  おわり", 12, 40);
      const blob = await new Promise((done) => canvas.toBlob(done, "image/png"));
      return [...new Uint8Array(await blob.arrayBuffer())];
    },
    read_pictures: async ({ ids }) => (
      await sleep(Number(q.get("slow") ?? 600)),
      { pictures: ids.length, blocks: 9 * ids.length, entries: 3 * ids.length, failed: [] }
    ),
    reinsert_project: () => ({
      workingPath: `${DIR}/working/brisa-e-o-farol-de-papel.rtsf`,
      apply: { applied: 11, keptOriginal: 7, ignoredGeneric: 0, relocated: 2, packedLeftOut: 0, noRoom: 0 },
      verification: {
        ok: true,
        checks: ["tamanho do arquivo confere", "cabeçalho íntegro", "11 strings relidas iguais à tradução"],
        problems: [],
      },
      forcedErrors: 0,
      skippedErrors: 2,
      pictures: { pictures: 1, drawn: 3, leftOut: [] },
    }),
    export_patch: () => ({
      patchPath: `${DIR}/exports/brisa-pt-BR.bps`,
      manifestPath: `${DIR}/exports/manifest.json`,
      csvPath: `${DIR}/exports/traducoes.csv`,
      patchFormat: "BPS",
      patchedSha256: "0".repeat(64),
      patchSize: 5120,
    }),
    export_entries: ({ path }) => path,
    codex_account_status: () => false,
    codex_login: () => null,
    codex_logout: () => null,
    codex_login_cancel: () => null,
  };

  if (q.get("tauri")) window.isTauri = true;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback(cb) {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    async invoke(cmd, args) {
      const h = handlers[cmd];
      if (!h) throw `mock: comando sem simulação: ${cmd}`;
      return h(args ?? {});
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };

  // auto=1: leva a tela do projeto ao estado "cheio" clicando nos botoes de verdade.
  // No fim marca <html data-ready> para quem for capturar a tela.
  const wait = async (find) => {
    for (let i = 0; i < 200; i++) {
      const el = find();
      if (el) return el;
      await sleep(50);
    }
    throw new Error("mock auto: elemento não apareceu");
  };
  const $ = (sel) => document.querySelector(sel);
  async function auto(mode) {
    const ready = () => (document.documentElement.dataset.ready = "1");
    await wait(() => $(".center-actions .primary"));
    if (mode === "home") return ready();
    if (mode === "drop") {
      await sleep(300); // o listener de arrastar registra depois do primeiro render
      emit("tauri://drag-enter", { paths: [SRC], position: { x: 0, y: 0 } });
      return ready();
    }
    if (["inspecting", "report", "created"].includes(mode)) {
      $(".center-actions .primary").click(); // Selecionar arquivo de jogo
      if (mode === "inspecting") return void (await wait(() => $(".hero-art.busy")), ready());
      await wait(() => $(".verdict") || $(".no-match"));
      if (mode === "created") {
        $(".actions .primary").click(); // Criar projeto
        await wait(() => $("code.path"));
      }
      return ready();
    }
    $(".center-actions button:not(.primary)").click(); // Abrir projeto
    await wait(() => $(".scan-controls"));
    if (mode === "project" || !entries.length) return void (await sleep(600), ready());
    if (mode === "tools") {
      // Campo controlado pelo React: o valor entra pelo setter nativo + evento.
      const set = (el, value, event) => {
        Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value").set.call(el, value);
        el.dispatchEvent(new Event(event, { bubbles: true }));
      };
      set($(".scan-controls select"), "table", "change"); // abre a busca relativa
      set(await wait(() => $(".panel input[type=text]")), "START", "input");
      await sleep(100);
      $(".panel .scan-controls button").click(); // Buscar
      $(".scan-run button:nth-child(2)").click(); // Raio-X
      $("details.panel > summary").click(); // Glossario
      await wait(() => $(".panel > table.strings") && $(".issues"));
      return ready();
    }
    await wait(() => $(".strings-wrap tr.row"));
    $(".results-bar button").click(); // Validar
    await wait(() => $(".strings-wrap tr.row.error"));
    document.querySelectorAll(".strings-wrap tr.row")[4].click(); // fala com erro
    $('h3[data-step="2"]').parentElement.querySelector(".actions .primary").click(); // Traduzir
    await wait(() => !$(".progress-row") && $('h3[data-step="2"]').parentElement.querySelector(".ok"));
    $('h3[data-step="3"]').parentElement.querySelector(".actions .primary").click(); // Reinserir
    await wait(() => $('h3[data-step="3"]').parentElement.querySelector(".ok"));
    await sleep(400);
    ready();
  }
  window.__mock = { emit, auto };
  if (q.get("auto")) addEventListener("load", () => void auto(q.get("auto")));
})();
