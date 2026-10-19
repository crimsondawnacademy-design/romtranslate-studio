#!/usr/bin/env python3
"""Acha onde uma palavra que aparece na tela mora no arquivo, sem saber a
tabela de letras do jogo (busca relativa).

Quase toda tabela guarda as letras em ordem alfabetica: "castle" aparece como
bytes com a mesma distancia entre si que c-a-s-t-l-e, seja qual for o byte do
'a'. Cada resultado diz o byte do 'A' e do 'a', onde a palavra aparece, o
trecho em volta lido com essa tabela e os bytes crus em volta (onde se ve o
cabecalho e o terminador das frases).

Exemplos:
  python3 -I achar_palavra.py jogo.nds castle
  python3 -I achar_palavra.py jogo.nds Skeleton --tbl letras.tbl
  python3 -I achar_palavra.py jogo.gba Potion --bytes 2   # letra de 16 bits
"""

import argparse
import sys

LETRAS = 26


def letras(palavra):
    """(maiuscula?, posicao no alfabeto) de cada letra."""
    out = []
    for c in palavra:
        if "A" <= c <= "Z":
            out.append((True, ord(c) - 65))
        elif "a" <= c <= "z":
            out.append((False, ord(c) - 97))
        else:
            sys.exit(f'so letras de A a Z, sem espaco, numero ou acento: "{palavra}"')
    distintas = max(len({i for u, i in out if u}), len({i for u, i in out if not u}))
    if len(out) < 4 or distintas < 3:
        sys.exit("use uma palavra com 4 letras ou mais e 3 letras diferentes num mesmo caso "
                 "(palavra curta ou repetitiva casa com lixo)")
    return out


def maior_trecho(palavra):
    """O maior trecho seguido de letras do mesmo caso: (inicio, tamanho)."""
    melhor, i = (0, 0), 0
    while i < len(palavra):
        j = i
        while j < len(palavra) and palavra[j][0] == palavra[i][0]:
            j += 1
        if j - i > melhor[1]:
            melhor = (i, j - i)
        i = j
    return melhor


def bases(fluxo, w, palavra):
    """Byte do 'A' e do 'a' se a palavra casa na posicao w, senao None."""
    achado = {True: None, False: None}
    for k, (maiuscula, idx) in enumerate(palavra):
        base = fluxo[w + k] - idx
        if base < 0 or base + LETRAS - 1 > 255:
            return None
        if achado[maiuscula] is None:
            achado[maiuscula] = base
        elif achado[maiuscula] != base:
            return None
    a, b = achado[True], achado[False]
    if a is not None and b is not None and abs(a - b) < LETRAS:
        return None
    return a, b


def buscar(dados, palavra, unidade):
    """{(base de 'A', base de 'a'): [posicoes no arquivo]}.

    Com letra de 16 bits (little-endian, alinhada), a busca corre no fluxo dos
    bytes baixos e exige o byte alto igual em toda a palavra.
    """
    # ponytail: 16 bits so little-endian e alinhado em 2; big-endian ou
    # desalinhado pede outra fatia (dados[1::2]) se aparecer um jogo assim.
    if unidade == 2:
        fluxo, alto = dados[0::2], dados[1::2]
    else:
        fluxo, alto = dados, None
    inicio, tamanho = maior_trecho(palavra)
    trecho = palavra[inicio:inicio + tamanho]
    achados = {}
    for base in range(256 - LETRAS + 1):
        agulha = bytes(base + idx for _, idx in trecho)
        p = fluxo.find(agulha)
        while p != -1:
            w = p - inicio
            if 0 <= w and w + len(palavra) <= len(fluxo):
                par = bases(fluxo, w, palavra)
                if par and (alto is None or len(set(alto[w:w + len(palavra)])) == 1):
                    if alto is not None:
                        hi = alto[w] << 8
                        par = tuple(None if b is None else hi | b for b in par)
                    achados.setdefault(par, []).append(w * unidade)
            p = fluxo.find(agulha, p + 1)
    return achados


def deslocamento(par):
    """Quanto o byte da letra difere do ASCII, se a tabela parece ASCII deslocado."""
    maiuscula, minuscula = par
    if maiuscula is not None and minuscula is not None:
        return maiuscula - 0x41 if minuscula - maiuscula == 0x20 else None
    return (maiuscula - 0x41) if maiuscula is not None else (minuscula - 0x61)


def ler(dados, pos, unidade, par):
    """Uma unidade lida com a tabela candidata, ou None."""
    if pos < 0 or pos + unidade > len(dados):
        return None
    valor = int.from_bytes(dados[pos:pos + unidade], "little")
    delta = deslocamento(par)
    if delta is not None:
        c = valor - delta
        return chr(c) if 0x20 <= c <= 0x7E else None
    for base, primeira in zip(par, (0x41, 0x61)):
        if base is not None and 0 <= valor - base < LETRAS:
            return chr(primeira + valor - base)
    return None


def tabela(par, unidade):
    """Linhas do .tbl: o ASCII imprimivel deslocado, ou so as letras achadas."""
    delta = deslocamento(par)
    if delta is not None:
        pares = [(c + delta, chr(c)) for c in range(0x20, 0x7F) if 0 <= c + delta < 256 ** unidade]
    else:
        pares = [(base + i, chr(primeira + i))
                 for base, primeira in zip(par, (0x41, 0x61)) if base is not None
                 for i in range(LETRAS)]
    return "".join(f"{v.to_bytes(unidade, 'little').hex().upper()}={c}\n" for v, c in pares)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("arquivo")
    ap.add_argument("palavra", help="palavra que aparece na tela, so letras (ex.: castle)")
    ap.add_argument("--bytes", type=int, choices=(1, 2), default=1, help="bytes por letra (padrao 1)")
    ap.add_argument("--inicio", type=lambda x: int(x, 0), default=0, help="so a partir desta posicao")
    ap.add_argument("--fim", type=lambda x: int(x, 0), default=None, help="so ate esta posicao")
    ap.add_argument("--tbl", help="grava a tabela do 1o resultado (ou do --resultado) neste arquivo")
    ap.add_argument("--resultado", type=int, default=1, help="qual resultado vira tabela (padrao 1)")
    args = ap.parse_args()

    palavra = letras(args.palavra)
    with open(args.arquivo, "rb") as f:
        tudo = f.read()
    inicio = args.inicio - args.inicio % args.bytes
    dados = tudo[inicio:args.fim]
    achados = buscar(dados, palavra, args.bytes)
    if not achados:
        print(f'"{args.palavra}" nao aparece com letras em ordem alfabetica. Tente outra palavra, '
              f"--bytes 2, ou o texto esta comprimido (procure blocos que descomprimem).")
        return 1
    ordem = sorted(achados.items(), key=lambda kv: -len(kv[1]))
    largura = 2 * args.bytes
    for n, (par, posicoes) in enumerate(ordem[:8], 1):
        a = "?" if par[0] is None else f"0x{par[0]:0{largura}X}"
        b = "?" if par[1] is None else f"0x{par[1]:0{largura}X}"
        delta = deslocamento(par)
        cara = f", parece ASCII com {delta:+#x}" if delta is not None else ""
        print(f"[{n}] 'A' = {a}, 'a' = {b}{cara}: {len(posicoes)} vez(es)")
        print("    em " + " ".join(f"0x{inicio + p:X}" for p in posicoes[:10])
              + (" ..." if len(posicoes) > 10 else ""))
        p = posicoes[0]
        u = args.bytes
        trecho = "".join(ler(dados, p + k * u, u, par) or "·" for k in range(-12, 48))
        print(f"    lido:  {trecho}")
        antes, depois = dados[max(0, p - 16):p], dados[p:p + 32]
        print(f"    bytes: {antes.hex(' ')} | {depois.hex(' ')}")
    if args.tbl:
        par = ordem[min(args.resultado, len(ordem)) - 1][0]
        with open(args.tbl, "w", encoding="utf-8") as f:
            f.write(tabela(par, args.bytes))
        print(f"tabela gravada em {args.tbl}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
