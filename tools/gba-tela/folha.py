# Junta fotos da tela lado a lado (em fileiras de 4), ampliadas 2x: python3 -I folha.py saida.png foto...
import sys
from PIL import Image

out, paths = sys.argv[1], sys.argv[2:]
shots = [Image.open(p).convert("RGB") for p in paths]
cols = min(4, len(shots))
rows = (len(shots) + cols - 1) // cols
w, h = 240, 160
sheet = Image.new("RGB", (cols * (w + 4) + 4, rows * (h + 4) + 4), (60, 0, 0))
for i, im in enumerate(shots):
    sheet.paste(im, (4 + (i % cols) * (w + 4), 4 + (i // cols) * (h + 4)))
sheet.resize((sheet.width * 2, sheet.height * 2), Image.NEAREST).save(out)
