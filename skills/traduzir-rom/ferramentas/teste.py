#!/usr/bin/env python3
"""Teste rapido das ferramentas, sobre arquivos sinteticos (nenhum byte de
jogo): roda cada uma como a IA roda, pela linha de comando.

  python3 -I skills/traduzir-rom/ferramentas/teste.py
"""

import os
import struct
import subprocess
import sys
import tempfile

AQUI = os.path.dirname(os.path.abspath(__file__))

# O banco do synth::make_profile_bank do app: letra = ASCII - 0x20, `01 00`
# abre, `EA` fecha, alinhado em 4, ponteiro com o endereco de memoria.
MEMORIA, ORIGEM, TABELA, BANCO = 0x02100000, 0x1000, 0x1100, 0x2000
FRASES = ["Bat", "[E3:10]The castle is quiet tonight.[E6]Too quiet.", "Restores 100 HP.", "Bat", "Skeleton"]


def codifica(texto):
    out, i = bytearray(), 0
    while i < len(texto):
        if texto[i] == "[":
            fim = texto.index("]", i)
            out += bytes.fromhex(texto[i + 1:fim].replace(":", ""))
            i = fim + 1
        else:
            out.append(ord(texto[i]) - 0x20)
            i += 1
    return bytes(out)


def banco():
    dados = bytearray(16 * 1024)
    pos = BANCO
    for n, frase in enumerate(FRASES):
        corpo = b"\x01\x00" + codifica(frase) + b"\xEA"
        dados[pos:pos + len(corpo)] = corpo
        struct.pack_into("<I", dados, TABELA + 4 * n, MEMORIA + pos - ORIGEM)
        pos = -(-(pos + len(corpo)) // 4) * 4
    # Letra de 16 bits, little-endian: unidade = 0x0100 + ASCII.
    dados[0x3000:0x3000 + 12] = b"".join(struct.pack("<H", 0x100 + ord(c)) for c in "Potion")
    return bytes(dados)


def fonte():
    dados = bytearray(0x200)
    letra_l = [0x80] * 7 + [0xFF]  # haste na esquerda, base embaixo
    dados[0x108:0x110] = bytes(letra_l)  # desenho 1 (o 0 fica vazio)
    return bytes(dados)


def nds():
    dados = bytearray(0x6000)
    dados[0:12] = b"TESTE".ljust(12, b"\0")
    dados[12:16] = b"ATST"
    struct.pack_into("<4I", dados, 0x20, 0x4000, 0x02000800, 0x02000000, 0x100)  # arm9
    struct.pack_into("<4I", dados, 0x30, 0x4200, 0x02380000, 0x02380000, 0x80)  # arm7
    struct.pack_into("<4I", dados, 0x40, 0x4600, 0x40, 0x4500, 3 * 8)  # FNT, FAT
    struct.pack_into("<2I", dados, 0x50, 0x4400, 32)  # overlays do arm9
    struct.pack_into("<8I", dados, 0x4400, 0, 0x0219E3E0, 0x100, 0, 0, 0, 0, 0)
    struct.pack_into("<6I", dados, 0x4500, 0x5000, 0x5100, 0x5100, 0x5140, 0x5140, 0x5180)
    struct.pack_into("<IHH", dados, 0x4600, 0x10, 1, 2)  # raiz: 2 pastas
    struct.pack_into("<IHH", dados, 0x4608, 0x20, 2, 0xF000)
    dados[0x4610:0x4610 + 14] = b"\x05a.bin\x84font\x01\xF0"
    dados[0x4620:0x4620 + 7] = b"\x06LD_u12"
    crc = 0xFFFF
    for b in dados[:0x15E]:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    struct.pack_into("<H", dados, 0x15E, crc)
    return bytes(dados)


def roda(ferramenta, *args, codigo=0):
    feito = subprocess.run([sys.executable, "-I", os.path.join(AQUI, ferramenta), *map(str, args)],
                           capture_output=True, text=True)
    assert feito.returncode == codigo, f"{ferramenta} {args}: {feito.returncode}\n{feito.stdout}{feito.stderr}"
    return feito.stdout + feito.stderr


def main():
    with tempfile.TemporaryDirectory() as tmp:
        caminho = lambda nome: os.path.join(tmp, nome)  # noqa: E731
        for nome, dados in (("banco.bin", banco()), ("fonte.bin", fonte()), ("teste.nds", nds())):
            with open(caminho(nome), "wb") as f:
                f.write(dados)

        for ferramenta in ("achar_palavra.py", "listar_frases.py", "achar_ponteiros.py",
                           "desenhar_fonte.py", "nds_arquivos.py"):
            assert "usage" in roda(ferramenta, "--help"), ferramenta

        # Busca relativa: acha a palavra, diz que e ASCII - 0x20 e grava a tabela.
        saida = roda("achar_palavra.py", caminho("banco.bin"), "castle", "--tbl", caminho("letras.tbl"))
        assert "'a' = 0x41, parece ASCII com -0x20" in saida, saida
        assert "The castle is quiet tonight." in saida, saida
        assert "00= \n" in open(caminho("letras.tbl"), encoding="utf-8").read()
        saida = roda("achar_palavra.py", caminho("banco.bin"), "Potion", "--bytes", "2")
        assert "'A' = 0x0141, 'a' = 0x0161" in saida and "em 0x3000" in saida, saida
        roda("achar_palavra.py", caminho("banco.bin"), "zzzz", codigo=1)  # repetitiva: recusada

        # Frases: sem o parametro, E3 aparece como pista; com ele, [E3:10].
        base = [caminho("banco.bin"), "--tbl", caminho("letras.tbl"), "--cabecalho", "0100",
                "--terminador", "EA", "--alinhamento", "4", "--inicio", "0x2000", "--fim", "0x2100"]
        saida = roda("listar_frases.py", *base)
        assert "5 frase(s)" in saida and "[E3]0The castle" in saida, saida
        saida = roda("listar_frases.py", *base, "--parametros", "E3=1")
        assert "0x002008    43  [E3:10]The castle is quiet tonight.[E6]Too quiet." in saida, saida

        # Ponteiros: os da tabela, e a tabela inteira pela faixa.
        mem = ["--memoria", hex(MEMORIA), "--origem", hex(ORIGEM)]
        saida = roda("achar_ponteiros.py", caminho("banco.bin"), *mem, "0x2000", "0x2002")
        assert "0x2000 (valor 0x02101000): 0x1100" in saida and "0x2002 (valor 0x02101002): nenhum" in saida
        saida = roda("achar_ponteiros.py", caminho("banco.bin"), *mem, "--alvo", "0x2000", "0x2100")
        assert "0x1100-0x1114: 5 ponteiros" in saida, saida

        # Fonte: o desenho 1 e um L (ponto aceso no canto de cima e embaixo a direita).
        roda("desenhar_fonte.py", caminho("fonte.bin"), "--inicio", "0x100", "--quantos", "2",
             "--colunas", "2", "--escala", "1", "--saida", caminho("fonte.png"))
        from PIL import Image
        img = Image.open(caminho("fonte.png"))
        x = 40 + 9  # margem + celula do desenho 0
        assert img.getpixel((x, 0)) == 0 and img.getpixel((x + 7, 7)) == 0 and img.getpixel((x + 7, 0)) != 0
        saida = roda("desenhar_fonte.py", caminho("fonte.bin"), "--inicio", "0x100", "--largura", "8,16",
                     "--saida", caminho("f.png"))
        assert os.path.exists(caminho("f-8.png")) and os.path.exists(caminho("f-16.png")), saida

        # DS: modulos com endereco de carga, arquivos com caminho, e --onde.
        saida = roda("nds_arquivos.py", caminho("teste.nds"), "--arquivos", "--onde", "0x5010")
        assert "overlay 0 (arm9)        0x00005000  0x00000100  0x0219E3E0" in saida, saida
        assert "font/LD_u12" in saida and "a.bin" in saida, saida
        assert "memoria 0x0219E3F0 (perfil: memoria = 0x0219E3E0, origem = 0x5000)" in saida, saida
        roda("nds_arquivos.py", caminho("banco.bin"), codigo=1)  # nao e .nds
    print("ferramentas ok")


if __name__ == "__main__":
    main()
