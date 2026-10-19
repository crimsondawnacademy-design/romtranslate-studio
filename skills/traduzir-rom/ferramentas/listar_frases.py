#!/usr/bin/env python3
"""Lista as frases candidatas de um trecho do arquivo, lidas com a tabela de
letras do jogo, um cabecalho e um terminador: a mesma leitura do perfil do
app (perfil.toml), pra conferir os valores antes de escreve-lo.

Byte que a tabela nao tem sai como codigo [XX]; com --parametros, o codigo
leva os bytes seguintes junto ([E3:10]). No fim, cada codigo achado com a
parte das vezes em que o byte seguinte nao e letra: perto de 100% e pista de
que o codigo leva parametro.

Exemplos:
  python3 -I listar_frases.py jogo.nds --tbl letras.tbl --terminador EA
  python3 -I listar_frases.py jogo.nds --tbl letras.tbl --cabecalho 0100 \\
      --terminador EA --alinhamento 4 --parametros E1=2,E3=1 --inicio 0x2000 --fim 0x2100
"""

import argparse
import sys

MAX_FRASE = 4096


def ler_tbl(caminho):
    """{bytes: texto} das linhas HEX=texto (comentario com #)."""
    tabela = {}
    with open(caminho, encoding="utf-8") as f:
        for linha in f:
            linha = linha.rstrip("\r\n")
            if not linha or linha.startswith("#") or "=" not in linha:
                continue
            chave, texto = linha.split("=", 1)
            try:
                tabela[bytes.fromhex(chave)] = texto
            except ValueError:
                continue
    return tabela


def parametros(texto):
    """'E1=2,E3=1' -> {0xE1: 2, 0xE3: 1}."""
    out = {}
    for parte in filter(None, (texto or "").split(",")):
        codigo, n = parte.split("=")
        out[int(codigo, 16)] = int(n)
    return out


def ler_frase(dados, pos, fim, tabela, maior, terminador, params):
    """(fim do corpo, [(texto, eh_codigo, bytes)]) ou None se nao ha terminador."""
    i, pedacos = pos, []
    while True:
        if i >= fim or i - pos > MAX_FRASE:
            return None
        if dados.startswith(terminador, i):
            return i, pedacos
        for n in range(min(maior, fim - i), 0, -1):
            texto = tabela.get(dados[i:i + n])
            if texto is not None:
                pedacos.append((texto, False, n))
                i += n
                break
        else:
            n = 1 + params.get(dados[i], 0)
            if i + n > fim:
                return None
            codigo = f"{dados[i]:02X}"
            if n > 1:
                codigo += ":" + dados[i + 1:i + n].hex().upper()
            pedacos.append((f"[{codigo}]", True, n))
            i += n


def cara_de_texto(pedacos, minimo):
    visivel = "".join(t for t, codigo, _ in pedacos if not codigo)
    letras = sum(c.isalpha() for c in visivel)
    return letras >= minimo and letras + visivel.count(" ") >= 0.7 * len(visivel)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("arquivo")
    ap.add_argument("--tbl", required=True, help="tabela de letras (.tbl), ex.: a do achar_palavra.py")
    ap.add_argument("--terminador", required=True, help="bytes que fecham a frase, em hex (ex.: EA ou 00)")
    ap.add_argument("--cabecalho", default="", help="bytes que abrem a frase, em hex (ex.: 0100)")
    ap.add_argument("--alinhamento", type=int, default=1, help="a frase comeca num multiplo disto")
    ap.add_argument("--parametros", help="codigo=bytes de parametro, ex.: E1=2,E3=1")
    ap.add_argument("--inicio", type=lambda x: int(x, 0), default=0)
    ap.add_argument("--fim", type=lambda x: int(x, 0), default=None)
    ap.add_argument("--min-letras", type=int, default=3, help="minimo de letras pra listar (padrao 3)")
    ap.add_argument("--todas", action="store_true", help="lista tambem o que nao tem cara de texto")
    ap.add_argument("--max", type=int, default=60, help="quantas frases mostrar (padrao 60)")
    args = ap.parse_args()

    tabela = ler_tbl(args.tbl)
    if not tabela:
        sys.exit(f"{args.tbl}: nenhuma linha HEX=texto")
    maior = max(map(len, tabela))
    letra = {k for k, v in tabela.items() if len(v) == 1 and v.isalpha()}
    cabecalho, terminador = bytes.fromhex(args.cabecalho), bytes.fromhex(args.terminador)
    params = parametros(args.parametros)
    with open(args.arquivo, "rb") as f:
        dados = f.read()
    fim = len(dados) if args.fim is None else min(args.fim, len(dados))
    alinhamento = max(1, args.alinhamento)

    frases, codigos = 0, {}
    pos = -(-args.inicio // alinhamento) * alinhamento
    while pos < fim:
        lido = dados.startswith(cabecalho, pos) and ler_frase(
            dados, pos + len(cabecalho), fim, tabela, maior, terminador, params)
        if not lido or lido[0] == pos + len(cabecalho):
            pos += alinhamento
            continue
        corpo_fim, pedacos = lido
        texto_ok = args.todas or cara_de_texto(pedacos, args.min_letras)
        if not texto_ok and cabecalho:
            # Dado que abre igual a uma frase pode engolir as de dentro.
            pos += alinhamento
            continue
        if texto_ok:
            frases += 1
            if frases <= args.max:
                texto = "".join(t for t, _, _ in pedacos)
                print(f"0x{pos:06X}  {corpo_fim - pos:4d}  {texto}")
            # Pista de parametro: o byte logo depois do codigo nao e letra.
            i = pos + len(cabecalho)
            for texto, codigo, n in pedacos:
                i += n
                if codigo:
                    vezes, sem_letra = codigos.get(texto[1:3], (0, 0))
                    codigos[texto[1:3]] = (vezes + 1, sem_letra + (dados[i:i + 1] not in letra))
        # A frase seguinte comeca depois do terminador, no alinhamento.
        pos = -(-(corpo_fim + len(terminador)) // alinhamento) * alinhamento

    print(f"\n{frases} frase(s) com cara de texto" + (f" (mostradas {args.max})" if frases > args.max else ""))
    if codigos:
        print("codigos fora da tabela (vezes, % com byte seguinte que nao e letra):")
        for cod, (vezes, sem_letra) in sorted(codigos.items(), key=lambda kv: -kv[1][0]):
            pista = "  <- talvez leve parametro" if vezes >= 3 and sem_letra == vezes and int(cod, 16) not in params else ""
            print(f"  [{cod}] {vezes:5d}  {100 * sem_letra // vezes:3d}%{pista}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
