#!/usr/bin/env python3
"""Mapa de um cartucho de Nintendo DS (.nds): ARM9, ARM7 e overlays com a
posicao no arquivo, o tamanho e o endereco de carga na memoria, e (com
--arquivos) cada arquivo do sistema de arquivos com o caminho.

O endereco de carga e a posicao do modulo sao o `memoria` e o `origem` do
perfil (perfil.toml): ponteiro dentro de um modulo guarda endereco de
memoria, nao posicao no arquivo.

Exemplos:
  python3 -I nds_arquivos.py jogo.nds
  python3 -I nds_arquivos.py jogo.nds --arquivos | grep -i font
  python3 -I nds_arquivos.py jogo.nds --onde 0x120A04   # em que modulo/arquivo cai
"""

import argparse
import struct
import sys


def u32(dados, pos):
    if pos + 4 > len(dados):
        raise ValueError(f"cabecalho corta em 0x{pos:X}")
    return struct.unpack_from("<I", dados, pos)[0]


def crc16(dados):
    """CRC-16 (0xA001) do cabecalho, o mesmo que o aparelho confere."""
    crc = 0xFFFF
    for b in dados:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


def fat(dados, base, tamanho, indice):
    """(inicio, fim) do arquivo `indice`, ou None."""
    pos = base + 8 * indice
    if indice < 0 or 8 * indice + 8 > tamanho or pos + 8 > len(dados):
        return None
    inicio, fim = struct.unpack_from("<II", dados, pos)
    return (inicio, fim) if inicio <= fim <= len(dados) else None


def modulos(dados):
    """[(nome, inicio, tamanho, carga, observacao)]."""
    out = [
        ("arm9", u32(dados, 0x20), u32(dados, 0x2C), u32(dados, 0x28), ""),
        ("arm7", u32(dados, 0x30), u32(dados, 0x3C), u32(dados, 0x38), ""),
    ]
    fat_pos, fat_tam = u32(dados, 0x48), u32(dados, 0x4C)
    for cpu, tabela in (("arm9", 0x50), ("arm7", 0x58)):
        pos, tam = u32(dados, tabela), u32(dados, tabela + 4)
        for k in range(tam // 32):
            e = pos + 32 * k
            if e + 32 > len(dados):
                break
            ident, carga, ram, _bss, _ini, _fim, arquivo, comp = struct.unpack_from("<8I", dados, e)
            onde = fat(dados, fat_pos, fat_tam, arquivo)
            if onde is None:
                continue
            obs = f"arquivo {arquivo}, {ram} bytes na memoria"
            if comp >> 24 & 1:
                obs += ", COMPRIMIDO (BLZ): o texto nao se le direto"
            out.append((f"overlay {ident} ({cpu})", onde[0], onde[1] - onde[0], carga, obs))
    return out


def arquivos(dados):
    """[(id, caminho, inicio, fim)] pelo FNT e o FAT."""
    fnt, fnt_tam = u32(dados, 0x40), u32(dados, 0x44)
    fat_pos, fat_tam = u32(dados, 0x48), u32(dados, 0x4C)
    if fnt + 8 > len(dados) or fnt_tam < 8:
        return []
    total_pastas = struct.unpack_from("<H", dados, fnt + 6)[0]
    out, visto = [], set()

    def pasta(indice, caminho):
        if indice in visto or indice >= total_pastas or fnt + 8 * indice + 8 > len(dados):
            return
        visto.add(indice)
        sub, primeiro, _ = struct.unpack_from("<IHH", dados, fnt + 8 * indice)
        p, ident = fnt + sub, primeiro
        while p < len(dados) and p < fnt + fnt_tam:
            n = dados[p]
            if n == 0:
                break
            nome = dados[p + 1:p + 1 + (n & 0x7F)].decode("latin-1")
            p += 1 + (n & 0x7F)
            if n & 0x80:
                if p + 2 > len(dados):
                    break
                filha = struct.unpack_from("<H", dados, p)[0] & 0x0FFF
                p += 2
                pasta(filha, caminho + nome + "/")
            else:
                onde = fat(dados, fat_pos, fat_tam, ident)
                if onde:
                    out.append((ident, caminho + nome, *onde))
                ident += 1

    pasta(0, "")
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("arquivo")
    ap.add_argument("--arquivos", action="store_true", help="lista tambem os arquivos do sistema de arquivos")
    ap.add_argument("--onde", type=lambda x: int(x, 0), help="diz em que modulo/arquivo cai esta posicao")
    args = ap.parse_args()

    with open(args.arquivo, "rb") as f:
        dados = f.read()
    if len(dados) < 0x200 or crc16(dados[:0x15E]) != struct.unpack_from("<H", dados, 0x15E)[0]:
        sys.exit("nao parece um .nds: o CRC do cabecalho (0x15E) nao confere")
    try:
        mods = modulos(dados)
        lista = arquivos(dados) if args.arquivos or args.onde is not None else []
    except ValueError as e:
        sys.exit(f"nao parece um .nds: {e}")
    titulo = dados[0:12].decode("ascii", "replace").rstrip("\0")
    print(f"titulo {titulo!r}, codigo {dados[12:16].decode('ascii', 'replace')!r}, {len(dados)} bytes")
    print("modulo                  posicao     tamanho     carga (memoria)")
    for nome, inicio, tamanho, carga, obs in mods:
        print(f"{nome:22s}  0x{inicio:08X}  0x{tamanho:08X}  0x{carga:08X}  {obs}")
    if args.arquivos:
        print(f"\n{len(lista)} arquivo(s):")
        for ident, caminho, inicio, fim in lista:
            print(f"  {ident:5d}  0x{inicio:08X}  {fim - inicio:9d}  {caminho}")
    if args.onde is not None:
        p = args.onde
        achou = False
        for nome, inicio, tamanho, carga, _ in mods:
            if inicio <= p < inicio + tamanho:
                print(f"0x{p:X} esta no {nome}: memoria 0x{carga + p - inicio:08X} "
                      f"(perfil: memoria = 0x{carga:08X}, origem = 0x{inicio:X})")
                achou = True
        for ident, caminho, inicio, fim in lista:
            if inicio <= p < fim:
                print(f"0x{p:X} esta no arquivo {ident} {caminho}, byte 0x{p - inicio:X} dele")
                achou = True
        if not achou:
            print(f"0x{p:X} nao esta em nenhum modulo nem arquivo")
    return 0


if __name__ == "__main__":
    sys.exit(main())
