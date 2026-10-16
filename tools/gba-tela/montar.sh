#!/bin/sh
# Baixa o mGBA 0.10.5 oficial, compila so a biblioteca e liga o roda.c nela.
# Tudo fica em tools/gba-tela/.mgba (fora do git). Uso: ./montar.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
src="$here/.mgba/src"
build="$here/.mgba/build"
if [ ! -d "$src" ]; then
	git clone -q --depth 1 --branch 0.10.5 https://github.com/mgba-emu/mgba.git "$src"
fi
cmake -S "$src" -B "$build" -DCMAKE_BUILD_TYPE=Release \
	-DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_PREFIX_PATH=/opt/homebrew \
	-DBUILD_QT=OFF -DBUILD_SDL=OFF -DBUILD_GL=OFF -DBUILD_GLES2=OFF -DBUILD_GLES3=OFF \
	-DBUILD_SHARED=OFF -DBUILD_STATIC=ON -DBUILD_LIBRETRO=OFF -DSKIP_LIBRARY=OFF \
	-DUSE_FFMPEG=OFF -DUSE_LIBZIP=OFF -DUSE_MINIZIP=OFF -DUSE_EPOXY=OFF -DUSE_SQLITE3=OFF \
	-DUSE_ELF=OFF -DUSE_LUA=OFF -DUSE_DISCORD_RPC=OFF -DUSE_EDITLINE=OFF -DUSE_LZMA=OFF \
	-DUSE_PNG=ON -DUSE_ZLIB=ON -DM_CORE_GB=OFF >/dev/null
log="$here/.mgba/montagem.log"
cmake --build "$build" --target mgba -j 8 >"$log" 2>&1 || { tail -20 "$log"; exit 1; }
cc -O2 -o "$here/.mgba/roda" "$here/roda.c" \
	-I"$src/include" -I"$build/include" \
	"$build/libmgba.a" -L/opt/homebrew/lib -lpng -lz -lm \
	-framework CoreFoundation
echo "pronto: $here/.mgba/roda"
