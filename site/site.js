// Site do RomTranslate Studio. Serve as duas paginas (os textos vem do HTML).
// Sem este arquivo a pagina continua completa e parada: os links de download
// caem na pagina de versoes do GitHub e nada fica escondido.
(() => {
  const root = document.documentElement;
  const $ = (sel, el = document) => el.querySelector(sel);
  const $$ = (sel, el = document) => [...el.querySelectorAll(sel)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  // ---------- Versao: VERSAO vem de versao.js ----------
  const BASE = `https://github.com/crimsondawnacademy-design/romtranslate-studio/releases/download/v${VERSAO}/`;
  for (const a of $$("a[data-arquivo]")) {
    const nome = a.dataset.arquivo.replaceAll("{v}", VERSAO);
    a.href = BASE + nome;
    const rotulo = $("[data-nome]", a);
    if (rotulo) rotulo.textContent = nome;
  }
  for (const el of $$("[data-versao]")) el.textContent = el.dataset.versao.replace("{v}", VERSAO);

  // ---------- Sistema de quem visita ----------
  // Celular e tablet ficam sem destaque: o app e de computador.
  const ua = navigator.userAgent;
  const plat = navigator.userAgentData?.platform || navigator.platform || "";
  const movel =
    /Android|iPhone|iPad|iPod/i.test(ua) || (/Mac/i.test(plat) && navigator.maxTouchPoints > 1);
  const os = movel
    ? ""
    : /Win/i.test(plat)
      ? "windows"
      : /Mac/i.test(plat)
        ? "macos"
        : /Linux|X11|CrOS/i.test(plat + ua)
          ? "linux"
          : "";
  const principal = $("a[data-rotulo]");
  if (os && principal) {
    root.dataset.os = os;
    const nome = { macos: "macOS", windows: "Windows", linux: "Linux" }[os];
    principal.firstElementChild.textContent = principal.dataset.rotulo.replace("{os}", nome);
    principal.setAttribute("href", principal.dataset.alvo.replace("{os}", os));
  }

  // ---------- Copiar (chave Pix e a linha do Terminal) ----------
  document.addEventListener("click", async (ev) => {
    const btn = ev.target.closest("button[data-copiar]");
    if (!btn) return;
    const status = $("[role=status]", btn.parentElement);
    status.textContent = "";
    try {
      await navigator.clipboard.writeText(btn.dataset.copiar);
      status.textContent = btn.dataset.ok;
    } catch {
      status.textContent = btn.dataset.falhou;
    }
  });

  // ---------- Movimento ----------
  const reduz = matchMedia("(prefers-reduced-motion: reduce)");
  const mexe = () => !reduz.matches && !root.classList.contains("still");

  // Botao de pausar: vale para a pagina inteira e fica guardado no navegador.
  const pausa = $(".motion-toggle");
  const guardado = () => {
    try {
      return localStorage.getItem("movimento") === "parado";
    } catch {
      return false;
    }
  };
  const aplicaPausa = (parado) => {
    root.classList.toggle("still", parado);
    pausa.textContent = parado ? pausa.dataset.retomar : pausa.dataset.pausar;
    pausa.setAttribute("aria-pressed", String(parado));
  };
  if (pausa) {
    pausa.hidden = false;
    aplicaPausa(guardado());
    pausa.addEventListener("click", () => {
      const parado = !root.classList.contains("still");
      aplicaPausa(parado);
      try {
        localStorage.setItem("movimento", parado ? "parado" : "ligado");
      } catch {
        /* sem armazenamento: vale so nesta visita */
      }
    });
  }

  // Fora da tela, a animacao da secao para (classe .off).
  const visivel = new WeakMap();
  const olho = new IntersectionObserver((entradas) => {
    for (const e of entradas) {
      visivel.set(e.target, e.isIntersecting);
      e.target.classList.toggle("off", !e.isIntersecting);
    }
  });
  for (const el of $$("[data-anim]")) olho.observe(el);

  // Entrada ao rolar. Quem ja esta na tela entra na hora; o resto, ao aparecer.
  const contar = (el) => {
    const fim = Number(el.dataset.count);
    const t0 = performance.now();
    const passo = (t) => {
      const p = Math.min(1, (t - t0) / 1400);
      el.textContent = Math.round(fim * (1 - (1 - p) ** 3)).toLocaleString(root.lang);
      if (p < 1) requestAnimationFrame(passo);
    };
    requestAnimationFrame(passo);
  };
  if (mexe()) {
    const entra = new IntersectionObserver(
      (entradas) => {
        for (const e of entradas) {
          if (!e.isIntersecting) continue;
          entra.unobserve(e.target);
          e.target.classList.add("in");
          if (mexe()) for (const n of $$("[data-count]", e.target)) contar(n);
        }
      },
      { rootMargin: "0px 0px -8% 0px" },
    );
    const alvos = $$(
      ".sec .head, .sec .card, .stat, .table, .window, .faq details, .downloads + .hint",
    );
    for (const el of alvos) {
      const irmaos = [...el.parentElement.children];
      el.style.setProperty("--d", `${Math.min(irmaos.indexOf(el), 7) * 70}ms`);
      el.classList.add("reveal");
      entra.observe(el);
    }
  }

  // Palco da abertura: a fala e digitada em ingles, o cartucho trabalha e a
  // traducao e digitada. As letras entram por CSS; aqui so se troca o texto.
  const palco = $(".stage[data-falas]");
  if (palco) {
    const falas = JSON.parse(palco.dataset.falas);
    const [antes, depois] = $$(".balloon", palco);
    const digita = (balao, texto) => {
      const linha = $(".line", balao);
      linha.textContent = "";
      let i = 0;
      for (const palavra of texto.split(" ")) {
        const w = document.createElement("span");
        w.className = "w";
        for (const letra of palavra) {
          const c = document.createElement("span");
          c.className = "c";
          c.style.setProperty("--i", i++);
          c.textContent = letra;
          w.append(c);
        }
        linha.append(w, " ");
        i++;
      }
      balao.classList.remove("wait", "pop");
      void balao.offsetWidth; // recomeca a animacao de entrada do balao
      balao.classList.add("pop");
      return i * 32 + 250;
    };
    const naTela = () => mexe() && !document.hidden && visivel.get(palco.closest("[data-anim]"));
    (async () => {
      await sleep(900);
      for (let n = 0; ; n = (n + 1) % falas.length) {
        while (!naTela()) await sleep(500);
        depois.classList.add("wait");
        await sleep(digita(antes, falas[n][0]) + 450);
        palco.classList.add("working");
        await sleep(900);
        palco.classList.remove("working");
        await sleep(digita(depois, falas[n][1]) + 2800);
      }
    })();
  }

  // Demonstracao: a tela do app acompanha o tamanho da pagina (--k) e a
  // moldura reage aos cliques la dentro. O iframe e da mesma origem.
  const vista = $(".window-view");
  const quadro = vista && $("iframe", vista);
  if (quadro) {
    const estreita = matchMedia("(max-width: 699px)");
    const ajusta = () => {
      const k = estreita.matches
        ? vista.clientWidth / 780
        : parseFloat(getComputedStyle(root).fontSize) / 16;
      vista.style.setProperty("--k", k.toFixed(4));
    };
    new ResizeObserver(ajusta).observe(vista);

    const janela = quadro.closest(".window");
    const passos = $$(".script .steps > li");
    janela.addEventListener("animationend", () => janela.classList.remove("hit"));
    const liga = () => {
      let doc;
      try {
        doc = quadro.contentDocument;
      } catch {
        return;
      }
      if (!doc || doc.documentElement.dataset.site) return;
      doc.documentElement.dataset.site = "1";
      doc.addEventListener(
        "click",
        (ev) => {
          if (mexe()) janela.classList.add("hit");
          // Os tres botoes do roteiro, pelos mesmos seletores do simulador do app.
          const b = ev.target.closest?.("button.primary");
          if (!b) return;
          const etapa = b.closest(".panel")?.querySelector("h3[data-step]")?.dataset.step;
          const feito = b.closest(".scan-run") ? 0 : etapa === "2" ? 1 : etapa === "3" ? 2 : -1;
          passos[feito]?.classList.add("done");
        },
        true,
      );
    };
    quadro.addEventListener("load", liga);
    liga();
  }
})();
