# Plataformas: o que o mercado pede e o que o app cobre

Medido em 20/10/2026 na [lista de pedidos de tradução do Data Crystal](https://datacrystal.tcrf.net/wiki/Translations_Request_List)
(cópia do Internet Archive de 13/07/2026 e das subpáginas; o site bloqueia leitura automática): **~2.300 jogos pedidos em 44 plataformas**.
Quase todos são **japoneses que nunca saíram traduzidos** — é esse o gargalo, e é nele que o app aposta.

## Cobertura

| Plataforma | Pedidos | No app | Lê japonês? |
|---|---:|---|---|
| PlayStation | 280 | extrai, regrava no lugar | Shift-JIS |
| Super Nintendo | 268 | extrai, regrava no lugar | por tabela/perfil |
| PlayStation 2 | 186 | extrai, regrava no lugar | Shift-JIS |
| PSP | 147 | extrai, regrava no lugar | Shift-JIS |
| NES / Famicom | 111 | extrai, regrava no lugar | por tabela/perfil |
| Saturn | 109 | — | — |
| Nintendo DS | 106 | extrai, arquivo cresce; MPT0 (Dragon Quest) | UTF-16, UTF-8 |
| Game Boy / Color | 104 | **novo (20/10)**: extrai, checksum refeito | Shift-JIS; kana por tabela/perfil |
| Game Boy Advance | 98 | extrai, ponteiros, LZ77, bancos | Shift-JIS e 16 bits na ordem JIS; fonte ganha acento |
| PC-9801/9821 | 80 | — | — |
| PC Engine CD | 77 | — | — |
| Mega Drive / Genesis (+ Pico 24) | 91 | **novo (20/10)**: extrai, checksum refeito | Shift-JIS; kana por tabela/perfil |
| PC (Windows/DOS/Mac) | 65 | fora do escopo (não é ROM) | — |
| WonderSwan / Color | 56 | **novo (20/10)**: extrai, checksum refeito | Shift-JIS; tabela/perfil |
| Dreamcast | 48 | — | — |
| 3DS | 47 | — (criptografado) | — |
| PlayStation 3 | 44 | — (criptografado) | — |
| Mega-CD | 37 | — | — |
| PS Vita | 37 | — (criptografado) | — |
| Xbox | 34 | — | — |
| PC Engine (HuCard) | 29 | **novo (20/10)**: extrai (reconhece pela extensão) | Shift-JIS; tabela/perfil |
| Wii | 28 | só detecta | — |
| N64 | 26 | **novo (20/10)**: extrai, CRC refeito (.z64) | Shift-JIS; tabela/perfil |
| X68000 | 26 | — | — |
| GameCube | 25 | extrai, regrava no lugar | Shift-JIS |
| PC-8801 | 23 | — | — |
| MSX | 21 | **novo (20/10)**: cartucho | Shift-JIS; tabela/perfil |
| PC-FX | 18 | — | — |
| FM Towns | 16 | — | — |
| Xbox 360 | 16 | — (criptografado) | — |
| Game Gear + Master System | 15 | **novo (20/10)**: extrai, checksum refeito | tabela/perfil |
| 3DO | 14 | — | — |
| Arcade | 9 | — | — |
| Famicom Disk System | 8 | **novo (20/10)**: extrai | tabela/perfil |
| Wii U | 5 | só detecta | — |
| Neo Geo Pocket | 3 | **novo (20/10)**: extrai | tabela/perfil |
| Outras (Amiga, Atari ST, CD-i, Neo Geo, celular) | 12 | — | — |

Antes de 20/10 o app cobria ~1.220 dos ~2.300 pedidos (53%). Com os cartuchos novos, ~1.580 (68%) — **todos experimentais**: os cartuchos novos foram conferidos só com arquivos sintéticos; nenhum jogo real dessas plataformas passou pelo app ainda.

## O gargalo do japonês, por etapa

1. **Achar o texto.** Discos (PS1, PS2, PSP, Saturn, PC Engine CD) quase sempre usam Shift-JIS: o app já lê. Cartucho de 8 e 16 bits usa tabela própria de kana: a skill `traduzir-rom` acha a tabela por uma palavra vista na tela e escreve o perfil.
2. **Caber.** Japonês diz em 2 bytes o que o português diz em 5 ou 6 letras. O que destrava: ponteiro em tabela (a frase muda de lugar), banco de mensagens remontado (GBA, DS), arquivo que cresce (DS). Sem isso, a frase fica presa no espaço do original.
3. **Escrever com a fonte do jogo.** Fonte japonesa não tem letra acentuada, e muitas desenham cada letra em largura inteira. Hoje o app desenha a letra acentuada na própria fonte em dois casos (GBA de 16 bits, fonte NFTR do DS). Falta: fonte de largura variável e fontes de 8 bits.
4. **Ver rodando.** Emulador sem janela: GBA e DS (`tools/gba-tela`, `tools/nds-tela`).

## Próximos passos, pelo tamanho do pedido

1. **Saturn (109) e Mega-CD (37):** disco ISO 9660 em setor Mode 1 — reaproveita o leitor de disco do PS1.
2. **PC Engine CD (77) e PC-FX (18):** trilha de dados crua, sem ISO 9660 na maioria; perfil do jogo por cima da trilha.
3. **PC-9801 (80), PC-8801 (23), X68000 (26):** imagens de disquete e HD (D88, FDI, HDI, DIM, XDF) com FAT12 por dentro.
4. **Dreamcast (48):** GDI/CDI, ISO 9660 na área de alta densidade.
5. **Plataformas criptografadas (3DS, PS3, Vita, Xbox 360, Wii U — 149):** o app não leva chave nenhuma. Só dá para trabalhar sobre arquivos que a pessoa já decriptou do próprio console.
