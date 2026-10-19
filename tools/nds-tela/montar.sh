#!/bin/sh
# Baixa o melonDS 1.1 oficial, compila so o nucleo (sem Qt/SDL, sem JIT, sem
# OpenGL) e liga o roda.cpp nele. Tudo fica em tools/nds-tela/.melonds (fora do
# git). Uso: ./montar.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
src="$here/.melonds/src"
build="$here/.melonds/build"
if [ ! -d "$src" ]; then
	git clone -q --depth 1 --branch 1.1 https://github.com/melonDS-emu/melonDS.git "$src"
fi
log="$here/.melonds/montagem.log"
cmake -S "$src" -B "$build" -DCMAKE_BUILD_TYPE=Release \
	-DBUILD_QT_SDL=OFF -DENABLE_JIT=OFF -DENABLE_OGLRENDERER=OFF \
	-DENABLE_GDBSTUB=OFF -DENABLE_LTO_RELEASE=OFF >"$log" 2>&1 || { tail -20 "$log"; exit 1; }
cmake --build "$build" --target core -j 8 >>"$log" 2>&1 || { tail -20 "$log"; exit 1; }
c++ -std=c++17 -O2 -fwrapv -Wall -o "$here/.melonds/roda" "$here/roda.cpp" \
	-isystem "$src/src" -isystem "$build/src" -I/opt/homebrew/include \
	"$build/src/libcore.a" "$build/src/teakra/src/libteakra.a" \
	-L/opt/homebrew/lib -lpng -lz
echo "pronto: $here/.melonds/roda"
