#!/bin/sh
# Roda o roteiro dentro da caixa de areia. Uso: ./rodar.sh <jogo.gba> <pasta> passo...
set -eu
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
rom=$1
shift 2
exec sandbox-exec -D SAIDA="$out" -f "$here/caixa.sb" "$here/.mgba/roda" "$rom" "$out" "$@"
