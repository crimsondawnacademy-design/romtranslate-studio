<p align="center">
  <img src="apps/desktop/src/assets/logo.svg" alt="RomTranslate Studio" width="120">
</p>

<h1 align="center">RomTranslate Studio</h1>

<p align="center">
  <b>Translate retro games with AI, on your own computer.</b><br>
  Drop a game file, review the translation, export a patch. Your ROM never leaves your machine.
</p>

<p align="center">
  <a href="https://github.com/crimsondawnacademy-design/romtranslate-studio/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/crimsondawnacademy-design/romtranslate-studio?include_prereleases&label=download"></a>
  <a href="https://github.com/crimsondawnacademy-design/romtranslate-studio/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/crimsondawnacademy-design/romtranslate-studio/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <a href="https://github.com/crimsondawnacademy-design/romtranslate-studio/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/crimsondawnacademy-design/romtranslate-studio?style=social"></a>
</p>

<p align="center">
  <a href="https://romtranslate-studio.pages.dev/en/"><b>Try it in your browser</b></a> ·
  <a href="README.pt-BR.md">Leia em português</a>
</p>

![RomTranslate Studio project screen with extracted lines, translations and validation status](docs/assets/screenshot.png)

<p align="center"><i>Demo project with made-up data. No real game content is stored in this repository. <a href="docs/assets/screenshot-full.png">See the whole screen.</a></i></p>

## What it does

1. **Detects** the platform of the file you drop in (11 formats, with the evidence it used).
2. **Extracts** the text, throwing away the bytes that only look like text.
3. **Translates** with the AI you choose, several batches at a time, telling the model how much room each line has.
4. **Validates** every line: placeholders, control codes, characters the game can write, and size. Lines that do not fit go back to the AI automatically.
5. **Reinserts** into a working copy and checks it. Your original file is never modified.
6. **Exports a patch** (IPS or BPS) that you can share instead of the game.

If you like the idea, a star helps other people find the project.

## Does it actually work?

It is alpha software, and this is what has been measured so far, on one commercial Game Boy Advance game from the maintainer's own cartridge dump (numbers only; no game data is in this repository):

| Measured | Result |
|---|---|
| Text segments translated and written back | 10,705 |
| Messages whose control codes stayed byte-identical | 3,699 of 3,699 |
| Bytes changed outside message text | 0 |
| Game boots with the translation, intro and first dialogue checked in an emulator | yes |
| AI lines that pass validation, ChatGPT through the Codex CLI, with automatic re-ask | 199 of 200 |
| Speed with 4 batches in parallel | 200 lines in 86 s |

What that does **not** prove: nobody has played that game to the end with the translation, only one real game has been through the whole process, and the accented build needed a character table and a text region set by hand. See [what it cannot do yet](#what-it-cannot-do-yet).

## Download

Get the installer for your system from the [latest release](https://github.com/crimsondawnacademy-design/romtranslate-studio/releases):

| System | File |
|---|---|
| macOS (Apple Silicon or Intel) | `.dmg` |
| Windows | `-setup.exe` or `.msi` |
| Linux | `.deb`, `.rpm` or `.AppImage` |

The binaries are not code-signed (this is a personal project and certificates are paid), so the system complains on first run:

- **macOS** says the app is damaged. After dragging it to Applications, run `xattr -cr "/Applications/RomTranslate Studio.app"`.
- **Windows** SmartScreen: "More info", then "Run anyway".
- **Linux**: `chmod +x RomTranslate*.AppImage`.

Only the macOS build has been opened and used by a person so far. The Windows and Linux installers are built by CI and still need someone to try them: if you do, please [open an issue](https://github.com/crimsondawnacademy-design/romtranslate-studio/issues) with what happened.

## Pick your AI

| Provider | Cost | Notes |
|---|---|---|
| **Ollama** | free, runs locally | Nothing leaves your computer. Small models struggle to keep lines short. |
| **ChatGPT subscription** via the Codex CLI | your existing plan, no API key | Sign in from inside the app. Subject to your plan's usage limit. |
| **Claude** (Anthropic API) | your API key | The app estimates the cost before the run and shows what was billed after. |
| Any **OpenAI-compatible** endpoint | your key | OpenRouter, LM Studio, a company gateway. |

Anything that is not local needs your explicit permission: the app tells you which text will be sent, and only the extracted lines go out, never the game file.

## Supported consoles

Every real platform is **experimental**. Games use different engines, compression and character tables, so there is no promise of universal support.

| Platform | Extract | Reinsert | Longer translations |
|---|---|---|---|
| Game Boy Advance | yes | in place, header checksum fixed | yes, for text reached by a pointer table |
| Nintendo DS | yes, ASCII and UTF-16 per file | in place, header CRC fixed | yes, the file grows and the FAT is updated |
| NES | yes, ASCII or a `.tbl` character table | in place | no |
| Super Nintendo | yes | in place, internal checksum fixed | no |
| GameCube | yes, per file | in place | no |
| PlayStation 1 (ISO, raw BIN, `.cue`) | yes, per file | in place, sector EDC/ECC regenerated | a little, into the last sector's slack |
| PlayStation 2 (including DVDs over 2 GiB) | yes, per file | in place | no |
| PSP | yes, per file | in place | no |
| Game Boy / Color, Mega Drive / Genesis (incl. Pico, 32X), Master System / Game Gear, WonderSwan / Color, Nintendo 64 (`.z64`) | yes, ASCII and Shift-JIS (new, checked with test files only) | in place, the cartridge checksum fixed when the original's was right | no |
| PC Engine (HuCard), MSX cartridge, Famicom Disk System, Neo Geo Pocket | yes, ASCII and Shift-JIS (new, checked with test files only) | in place | no |
| Saturn, Mega-CD (ISO, raw BIN) | yes, per file, ASCII and Shift-JIS (new, checked with test files only) | in place, sector EDC/ECC regenerated | no |
| Wii, Wii U | detection only | no | no |

"In place" means the translation uses the room of the original line. Which platforms the translation-request lists ask for, and what is still missing, is in [docs/PLATAFORMAS.md](docs/PLATAFORMAS.md) (Portuguese). The full matrix, with what each column means and the known limits, is in [docs/DETALHES.md](docs/DETALHES.md) (Portuguese).

## What it cannot do yet

- Compressed text: on Game Boy Advance the app extracts and rewrites the text inside LZ77 blocks, and each translation must fit the space of its original inside the block. This was checked with test files only: no real game with compressed text has been through it. On DS compressed text is only counted, and other compressors are not recognized.
- Japanese: the app reads Shift-JIS text (the most common form on disc games) and, on Game Boy Advance, text stored as 16-bit codes in the order of the standard Japanese table, building the letter table by itself (this ships in the next version). One real Japanese game was measured: the app reads 12,296 of its messages, 96.6% of the Japanese in the cartridge (the few kanji come out wrong, because the game has its own list). Writing the translation back is still the bottleneck: each letter takes the space of one Japanese character, lines cannot move and there are no accents, so Japanese to Portuguese or English rarely fits. Nobody has seen such a translation running. Words written only in kanji are not read in Shift-JIS, and cartridge games whose table is not in the standard order still need that table.
- It does not edit fonts or text drawn as graphics (title screens, some menus).
- Most text can only be as long as the original. Growing it needs a pointer table the app recognises; tables declared by the user are on the roadmap.
- Choosing the text region and the character table for a cartridge game is still manual work.

## Translate a game the app cannot read, with an AI

When the automatic extraction does not find a game's dialogue, an AI coding assistant can study the file for you: find the letter table, where each line starts and ends, the pointers and the font's accented letters, and write it down as a small game profile (`perfil.toml`, see [docs/PERFIL.md](docs/PERFIL.md)). The app then translates, validates, writes back and exports the patch. The step-by-step guide is [skills/traduzir-rom/SKILL.md](skills/traduzir-rom/SKILL.md) (Portuguese), with small Python helpers next to it. Claude Code picks it up when opened in this folder; Codex and other assistants are pointed to it by [AGENTS.md](AGENTS.md). You do not need to be a romhacker, but you do need a game file you own: share only the patch and the profile.

## For tinkerers

- **Relative search** finds a game's custom character table from one word you know is in it, and writes the `.tbl` for you.
- **X-ray** shows what the app cannot read: compressed blocks, dense areas, how many lines can grow.
- **Command line**: every step also runs without the window, from source: `cargo run -p romtranslate-cli -- --help`. The commands are listed in [docs/DETALHES.md](docs/DETALHES.md#sem-a-janela-cli).
- **Write an adapter** for a new platform or engine: [docs/ADAPTERS.md](docs/ADAPTERS.md).

## Build from source

You need stable Rust, Node 20 or newer, pnpm and the [Tauri 2 system dependencies](https://tauri.app/start/prerequisites/).

```bash
git clone https://github.com/crimsondawnacademy-design/romtranslate-studio.git
cd romtranslate-studio
pnpm install
pnpm dev
```

To try it without a game, `cargo run -p romtranslate-core --example gen_fixtures` writes synthetic test files to `fixtures/generated/`.

## Rules of the house

- **Patch first.** Share the patch, never the game. The app works on files you already own and does not download ROMs.
- **Local first.** The game file stays on your computer.
- **No commercial ROM, BIOS or key** in this repository, in tests or in issues. Fixtures are synthetic.
- **No claim without a test.** A capability is only listed when a round trip proves it.

## Contributing

Bug reports, a game that did not work, a platform you want: [issues](https://github.com/crimsondawnacademy-design/romtranslate-studio/issues) are open, and [CONTRIBUTING.md](CONTRIBUTING.md) explains how to send code. Please never attach a commercial ROM.

## Support the project

RomTranslate Studio is free, MIT-licensed and made in spare time. If it helped you:

- **Star the repository.** It costs nothing and it is what makes the project visible.
- **Pix (Brazil):** key `13982286871` (phone).

## License

[MIT](LICENSE). The license covers this software only: it gives no rights over any game, trademark or translation of someone else's work.
