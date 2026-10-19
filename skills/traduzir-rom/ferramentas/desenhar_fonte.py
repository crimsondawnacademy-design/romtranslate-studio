#!/usr/bin/env python3
"""Desenha a fonte do jogo num PNG: cada desenho de letra lido de uma posicao
do arquivo, 1 bit por ponto, numa grade com o codigo de cada linha na
margem. E assim que se ve se a fonte tem as letras acentuadas e qual codigo
cada uma tem.

Nao sabe a largura? Passe varias (--largura 8,12,16): sai um PNG por largura,
e a certa e a que mostra letras inteiras, sem desenho torto.

Exemplos:
  python3 -I desenhar_fonte.py jogo.nds --inicio 0x3A0000 --largura 8,12,16 --saida fonte.png
  python3 -I desenhar_fonte.py jogo.gba --inicio 0xCE39F8 --largura 16 --altura 10 \\
      --pular 2 --passo 22 --primeiro 0x20 --saida fonte.png

Precisa do Pillow (pip install pillow).
"""

import argparse
import sys

try:
    from PIL import Image, ImageDraw
except ImportError:
    sys.exit("precisa do Pillow: pip install pillow")

MARGEM = 40  # espaco do codigo de cada linha, em pontos da imagem final


def desenho(dados, pos, largura, altura, bytes_linha, msb):
    """Pontos acesos do desenho que comeca em pos: lista de (x, y)."""
    pontos = []
    for y in range(altura):
        linha = dados[pos + y * bytes_linha:pos + (y + 1) * bytes_linha]
        if len(linha) < bytes_linha:
            break
        for x in range(largura):
            # msb: o ponto da esquerda e o bit mais alto de cada byte.
            byte, bit = divmod(x, 8)
            if byte < bytes_linha and (linha[byte] >> (7 - bit if msb else bit)) & 1:
                pontos.append((x, y))
    return pontos


def folha(dados, args, largura):
    # ponytail: so 1 bit por ponto e desenho linha a linha; fonte de 2/4 bits
    # ou em quadros 8x8 (fonte de DS em NFTR, GBA em tiles) pede outro leitor.
    altura = args.altura or largura
    bytes_linha = args.bytes_linha or -(-largura // 8)
    passo = args.passo or args.pular + bytes_linha * altura
    colunas, escala = args.colunas, args.escala
    linhas = -(-args.quantos // colunas)
    celula = (largura + 1, altura + 1)
    img = Image.new("L", (MARGEM + colunas * celula[0] * escala, linhas * celula[1] * escala), 255)
    tela = ImageDraw.Draw(img)
    for n in range(args.quantos):
        pos = args.inicio + n * passo + args.pular
        if pos >= len(dados):
            break
        cx = MARGEM + (n % colunas) * celula[0] * escala
        cy = (n // colunas) * celula[1] * escala
        tela.rectangle([cx, cy, cx + largura * escala, cy + altura * escala], fill=235)
        for x, y in desenho(dados, pos, largura, altura, bytes_linha, args.bits == "msb"):
            tela.rectangle([cx + x * escala, cy + y * escala,
                            cx + (x + 1) * escala - 1, cy + (y + 1) * escala - 1], fill=0)
        if n % colunas == 0:
            tela.text((2, cy + 1), f"{args.primeiro + n:X}", fill=0)
    return img


def main():
    num = lambda x: int(x, 0)  # noqa: E731
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("arquivo")
    ap.add_argument("--inicio", type=num, required=True, help="posicao do primeiro desenho no arquivo")
    ap.add_argument("--largura", default="8", help="largura em pontos; varias com virgula (8,12,16)")
    ap.add_argument("--altura", type=int, help="altura em pontos (padrao: igual a largura)")
    ap.add_argument("--bytes-linha", type=int, help="bytes de cada linha do desenho (padrao: largura/8 pra cima)")
    ap.add_argument("--pular", type=int, default=0, help="bytes antes do desenho em cada letra (ex.: o codigo)")
    ap.add_argument("--passo", type=int, help="bytes de uma letra ate a seguinte (padrao: pular + desenho)")
    ap.add_argument("--bits", choices=("msb", "lsb"), default="msb", help="ordem dos pontos no byte")
    ap.add_argument("--quantos", type=int, default=256, help="quantas letras (padrao 256)")
    ap.add_argument("--primeiro", type=num, default=0, help="codigo da primeira letra, pra margem")
    ap.add_argument("--colunas", type=int, default=16)
    ap.add_argument("--escala", type=int, default=3)
    ap.add_argument("--saida", required=True, help="PNG de saida")
    args = ap.parse_args()

    with open(args.arquivo, "rb") as f:
        dados = f.read()
    if args.inicio >= len(dados):
        sys.exit(f"--inicio 0x{args.inicio:X} passa do fim do arquivo ({len(dados)} bytes)")
    larguras = [int(x) for x in args.largura.split(",")]
    for largura in larguras:
        saida = args.saida
        if len(larguras) > 1:
            base, _, ext = saida.rpartition(".")
            saida = f"{base or saida}-{largura}.{ext if base else 'png'}"
        folha(dados, args, largura).save(saida)
        print(saida)
    return 0


if __name__ == "__main__":
    sys.exit(main())
