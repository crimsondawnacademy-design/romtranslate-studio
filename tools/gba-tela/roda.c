// Roda um jogo de GBA sem janela, seguindo um roteiro de teclas, e grava
// fotos da tela, copias da memoria e estados salvos. Ferramenta de
// conferencia do processo de traducao: nao faz parte do app.
//
// Uso: roda <jogo.gba> <pasta> [--sav arquivo.sav] [--estado arquivo.ss] passo...
//
// Passos:
//   N            anda N quadros sem tecla
//   TECLA[*n]    aperta e solta a tecla, n vezes (A B SELECT START RIGHT
//                LEFT UP DOWN R L)
//   hold=TECLA:N segura a tecla por N quadros
//   shot=nome    foto da tela em <pasta>/nome.png
//   dump=nome    memoria em <pasta>/nome-ewram.bin, -iwram.bin e -vram.bin
//   save=nome    estado salvo em <pasta>/nome.ss
//   poke=END:VAL:BYTES  escreve VAL (1, 2 ou 4 bytes) no endereco END
//
// O jogo, o .sav e o estado sao so lidos; o .sav e copiado pra <pasta>
// antes de abrir, porque o jogo escreve nele.

// As opcoes com que a biblioteca foi compilada: mudam o formato das estruturas.
#include <mgba/flags.h>
#include <mgba/core/core.h>
#include <mgba/core/log.h>
#include <mgba/core/serialize.h>
#include <mgba-util/vfs.h>

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *KEYS[] = {"A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"};

static void quiet(struct mLogger *logger, int category, enum mLogLevel level, const char *format, va_list args) {
	(void) logger;
	(void) category;
	(void) level;
	(void) format;
	(void) args;
}

static int key_bit(const char *name, size_t len) {
	for (int i = 0; i < 10; ++i) {
		if (strlen(KEYS[i]) == len && strncmp(KEYS[i], name, len) == 0) {
			return i;
		}
	}
	return -1;
}

static unsigned long frames = 0;

static void run(struct mCore *core, int keys, unsigned long n) {
	core->setKeys(core, keys);
	for (unsigned long i = 0; i < n; ++i) {
		core->runFrame(core);
		++frames;
	}
	core->setKeys(core, 0);
}

static char *in_dir(const char *dir, const char *name, const char *suffix) {
	size_t len = strlen(dir) + strlen(name) + strlen(suffix) + 2;
	char *path = malloc(len);
	snprintf(path, len, "%s/%s%s", dir, name, suffix);
	return path;
}

static int dump(struct mCore *core, const char *dir, const char *name) {
	static const struct {
		const char *suffix;
		uint32_t base;
		uint32_t size;
	} regions[] = {
		{"-ewram.bin", 0x02000000, 0x40000},
		{"-iwram.bin", 0x03000000, 0x8000},
		{"-vram.bin", 0x06000000, 0x18000},
	};
	for (size_t r = 0; r < sizeof(regions) / sizeof(regions[0]); ++r) {
		char *path = in_dir(dir, name, regions[r].suffix);
		FILE *f = fopen(path, "wb");
		free(path);
		if (!f) {
			return -1;
		}
		for (uint32_t i = 0; i < regions[r].size; ++i) {
			fputc(core->busRead8(core, regions[r].base + i), f);
		}
		fclose(f);
	}
	return 0;
}

int main(int argc, char **argv) {
	if (argc < 3) {
		fprintf(stderr, "uso: roda <jogo.gba> <pasta> [--sav arquivo] [--estado arquivo] passo...\n");
		return 2;
	}
	const char *rom = argv[1];
	const char *dir = argv[2];
	int first = 3;
	const char *sav = NULL;
	const char *state = NULL;
	while (first + 1 < argc && strncmp(argv[first], "--", 2) == 0) {
		if (strcmp(argv[first], "--sav") == 0) {
			sav = argv[first + 1];
		} else if (strcmp(argv[first], "--estado") == 0) {
			state = argv[first + 1];
		} else {
			fprintf(stderr, "opcao desconhecida: %s\n", argv[first]);
			return 2;
		}
		first += 2;
	}

	static struct mLogger logger = {.log = quiet};
	mLogSetDefaultLogger(&logger);

	struct mCore *core = mCoreFind(rom);
	if (!core || !core->init(core)) {
		fprintf(stderr, "nao reconheci o jogo: %s\n", rom);
		return 1;
	}
	mCoreInitConfig(core, NULL);
	unsigned width, height;
	core->desiredVideoDimensions(core, &width, &height);
	color_t *video = calloc((size_t) width * height, BYTES_PER_PIXEL);
	core->setVideoBuffer(core, video, width);
	if (!mCoreLoadFile(core, rom)) {
		fprintf(stderr, "nao abri o jogo: %s\n", rom);
		return 1;
	}
	if (sav) {
		// Copia pra pasta: o jogo grava no .sav, e o original fica como esta.
		char *copy = in_dir(dir, "jogo", ".sav");
		FILE *in = fopen(sav, "rb");
		FILE *out = fopen(copy, "wb");
		if (!in || !out) {
			fprintf(stderr, "nao copiei o .sav: %s\n", sav);
			return 1;
		}
		int c;
		while ((c = fgetc(in)) != EOF) {
			fputc(c, out);
		}
		fclose(in);
		fclose(out);
		core->loadSave(core, VFileOpen(copy, O_RDWR));
		free(copy);
	}
	core->reset(core);
	if (state) {
		struct VFile *vf = VFileOpen(state, O_RDONLY);
		if (!vf || !mCoreLoadStateNamed(core, vf, SAVESTATE_SAVEDATA | SAVESTATE_RTC)) {
			fprintf(stderr, "nao carreguei o estado: %s\n", state);
			return 1;
		}
		vf->close(vf);
	}

	for (int i = first; i < argc; ++i) {
		const char *step = argv[i];
		if (strncmp(step, "shot=", 5) == 0) {
			char *path = in_dir(dir, step + 5, ".png");
			struct VFile *vf = VFileOpen(path, O_CREAT | O_TRUNC | O_WRONLY);
			free(path);
			if (!vf) {
				fprintf(stderr, "nao gravei a foto: %s\n", step);
				return 1;
			}
			mCoreTakeScreenshotVF(core, vf);
			vf->close(vf);
		} else if (strncmp(step, "dump=", 5) == 0) {
			if (dump(core, dir, step + 5) < 0) {
				fprintf(stderr, "nao gravei a memoria: %s\n", step);
				return 1;
			}
		} else if (strncmp(step, "save=", 5) == 0) {
			char *path = in_dir(dir, step + 5, ".ss");
			struct VFile *vf = VFileOpen(path, O_CREAT | O_TRUNC | O_RDWR);
			free(path);
			if (!vf || !mCoreSaveStateNamed(core, vf, SAVESTATE_SAVEDATA | SAVESTATE_RTC)) {
				fprintf(stderr, "nao gravei o estado: %s\n", step);
				return 1;
			}
			vf->close(vf);
		} else if (strncmp(step, "poke=", 5) == 0) {
			unsigned long address, value, bytes;
			if (sscanf(step + 5, "%li:%li:%lu", &address, &value, &bytes) != 3) {
				fprintf(stderr, "poke invalido: %s\n", step);
				return 2;
			}
			if (bytes == 1) {
				core->busWrite8(core, address, value);
			} else if (bytes == 2) {
				core->busWrite16(core, address, value);
			} else {
				core->busWrite32(core, address, value);
			}
		} else if (strncmp(step, "hold=", 5) == 0) {
			const char *colon = strchr(step + 5, ':');
			int bit = colon ? key_bit(step + 5, colon - (step + 5)) : -1;
			if (bit < 0) {
				fprintf(stderr, "hold invalido: %s\n", step);
				return 2;
			}
			run(core, 1 << bit, strtoul(colon + 1, NULL, 10));
		} else if (step[0] >= '0' && step[0] <= '9') {
			run(core, 0, strtoul(step, NULL, 10));
		} else {
			const char *star = strchr(step, '*');
			size_t len = star ? (size_t) (star - step) : strlen(step);
			int bit = key_bit(step, len);
			if (bit < 0) {
				fprintf(stderr, "passo desconhecido: %s\n", step);
				return 2;
			}
			unsigned long times = star ? strtoul(star + 1, NULL, 10) : 1;
			for (unsigned long t = 0; t < times; ++t) {
				run(core, 1 << bit, 4);
				run(core, 0, 8);
			}
		}
	}
	printf("quadros: %lu\n", frames);
	core->deinit(core);
	free(video);
	return 0;
}
