#!/usr/bin/env python3
"""Acha os ponteiros de 32 bits (little-endian, alinhados em 4) que levam a
frases do arquivo, com a conta do perfil do app:

    valor do ponteiro = memoria + (posicao da frase no arquivo - origem)

Cartucho de GBA: memoria 0x08000000, origem 0 (o padrao). Modulo de DS
(ARM9 ou overlay): memoria = endereco de carga, origem = posicao do modulo
no cartucho (os dois saem do nds_arquivos.py).

Dois jeitos:
  posicoes  -> onde mora o ponteiro de cada frase dada
  --alvo    -> tabelas de ponteiros (palavras seguidas) que levam a uma faixa

Exemplos:
  python3 -I achar_ponteiros.py jogo.gba 0x7A1230 0x7A1250
  python3 -I achar_ponteiros.py jogo.nds --memoria 0x0219E3E0 --origem 0x120000 0x121004
  python3 -I achar_ponteiros.py jogo.nds --memoria 0x0219E3E0 --origem 0x120000 \\
      --alvo 0x120000 0x138000
"""

import argparse
import struct
import sys


def faixas(args, tamanho):
    lista = args.procurar or [[0, tamanho]]
    return [(max(0, a) // 4 * 4, min(b, tamanho)) for a, b in lista]


def main():
    num = lambda x: int(x, 0)  # noqa: E731
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("arquivo")
    ap.add_argument("posicoes", nargs="*", type=num, help="posicao de cada frase no arquivo (do cabecalho)")
    ap.add_argument("--memoria", type=num, default=0x08000000, help="endereco de memoria do trecho (padrao GBA)")
    ap.add_argument("--origem", type=num, default=0, help="posicao do trecho no arquivo (padrao 0)")
    ap.add_argument("--procurar", type=num, nargs=2, action="append", metavar=("INICIO", "FIM"),
                    help="so procura ponteiros nesta faixa (repetivel; padrao: o arquivo inteiro)")
    ap.add_argument("--alvo", type=num, nargs=2, metavar=("INICIO", "FIM"),
                    help="lista as tabelas de ponteiros que levam a esta faixa do arquivo")
    ap.add_argument("--minimo", type=int, default=3, help="com --alvo: palavras seguidas pra contar como tabela")
    args = ap.parse_args()
    if not args.posicoes and not args.alvo:
        ap.error("de as posicoes das frases ou --alvo INICIO FIM")

    with open(args.arquivo, "rb") as f:
        dados = f.read()
    onde = faixas(args, len(dados))

    for pos in args.posicoes:
        valor = args.memoria + pos - args.origem
        if not 0 <= valor < 2 ** 32:
            print(f"0x{pos:X}: valor 0x{valor:X} fora de 32 bits (memoria/origem trocadas?)")
            continue
        agulha = struct.pack("<I", valor)
        achados = []
        for a, b in onde:
            k = dados.find(agulha, a, b)
            while k != -1:
                if k % 4 == 0:
                    achados.append(k)
                k = dados.find(agulha, k + 1, b)
        lista = " ".join(f"0x{k:X}" for k in achados[:12]) or "nenhum"
        print(f"0x{pos:X} (valor 0x{valor:08X}): {lista}" + (" ..." if len(achados) > 12 else ""))

    if args.alvo:
        inicio, fim = args.alvo
        tabelas, atual = [], []
        for a, b in onde:
            # ponytail: varre palavra a palavra em Python (~16 M por segundo e
            # pouco); cartucho de 64 MiB leva uns segundos. --procurar encurta.
            for k in range(a, b - 3, 4):
                valor = int.from_bytes(dados[k:k + 4], "little")
                destino = valor - args.memoria + args.origem
                if inicio <= destino < fim:
                    if atual and k != atual[-1][0] + 4:
                        tabelas.append(atual)
                        atual = []
                    atual.append((k, destino))
            if atual:
                tabelas.append(atual)
                atual = []
        tabelas = [t for t in tabelas if len(t) >= args.minimo]
        total = sum(map(len, tabelas))
        print(f"{len(tabelas)} tabela(s) com {args.minimo}+ ponteiros seguidos para "
              f"0x{inicio:X}-0x{fim:X} ({total} ponteiros):")
        for t in sorted(tabelas, key=len, reverse=True)[:20]:
            destinos = " ".join(f"0x{d:X}" for _, d in t[:4])
            print(f"  0x{t[0][0]:X}-0x{t[-1][0] + 4:X}: {len(t)} ponteiros -> {destinos}"
                  + (" ..." if len(t) > 4 else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
