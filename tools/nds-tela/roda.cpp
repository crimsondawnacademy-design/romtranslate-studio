// Roda um jogo de Nintendo DS sem janela, seguindo um roteiro de teclas, e
// grava fotos das duas telas. Ferramenta de conferencia do processo de
// traducao: nao faz parte do app.
//
// Uso: roda <jogo.nds> <pasta> [--sav arquivo.sav] passo...
//
// Passos:
//   N              anda N quadros sem tecla
//   TECLA[*n]      aperta e solta a tecla, n vezes (A B SELECT START RIGHT
//                  LEFT UP DOWN R L X Y)
//   hold=TECLA:N   segura a tecla por N quadros
//   touch=X,Y[:N]  toca a tela de baixo em X,Y (0-255, 0-191) por N quadros
//   shot=nome      foto das duas telas (cima em cima) em <pasta>/nome.png
//
// Liga com a BIOS livre do melonDS e boot direto: nada de BIOS ou firmware
// de verdade. O jogo e o .sav sao so lidos; o que o jogo gravar vai pra
// <pasta>/jogo.sav.

#include "NDS.h"
#include "NDSCart.h"
#include "Args.h"
#include "GPU.h"
#include "Platform.h"

#include <png.h>

#include <chrono>
#include <condition_variable>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>
#include <thread>
#include <unistd.h>
#include <vector>

using namespace melonDS;

// --- o que o nucleo pede da plataforma -------------------------------------

namespace melonDS::Platform
{

static std::string save_path;
static const u8 *save_data = nullptr;
static u32 save_len = 0;

void SignalStop(StopReason reason, void *) { fprintf(stderr, "o jogo parou (motivo %d)\n", (int) reason); }

std::string GetLocalFilePath(const std::string &filename) { return filename; }

FileHandle *OpenFile(const std::string &path, FileMode mode)
{
	const char *how = "rb";
	if (mode & FileMode::Append) {
		how = (mode & FileMode::Read) ? "a+b" : "ab";
	} else if (mode & FileMode::Write) {
		if (mode & FileMode::Preserve) {
			if (access(path.c_str(), F_OK) != 0) {
				if (mode & FileMode::NoCreate) {
					return nullptr;
				}
				if (FILE *f = fopen(path.c_str(), "wb")) {
					fclose(f);
				}
			}
			how = "r+b";
		} else {
			how = (mode & FileMode::Read) ? "w+b" : "wb";
		}
	}
	return (FileHandle *) fopen(path.c_str(), how);
}

FileHandle *OpenLocalFile(const std::string &path, FileMode mode) { return OpenFile(path, mode); }
bool FileExists(const std::string &name) { return access(name.c_str(), F_OK) == 0; }
bool LocalFileExists(const std::string &name) { return FileExists(name); }
bool CheckFileWritable(const std::string &path) { return access(path.c_str(), W_OK) == 0; }
bool CheckLocalFileWritable(const std::string &path) { return CheckFileWritable(path); }
bool CloseFile(FileHandle *f) { return fclose((FILE *) f) == 0; }
bool IsEndOfFile(FileHandle *f) { return feof((FILE *) f) != 0; }
bool FileReadLine(char *str, int count, FileHandle *f) { return fgets(str, count, (FILE *) f) != nullptr; }
u64 FilePosition(FileHandle *f) { return ftell((FILE *) f); }
void FileRewind(FileHandle *f) { rewind((FILE *) f); }
u64 FileRead(void *data, u64 size, u64 count, FileHandle *f) { return fread(data, size, count, (FILE *) f); }
bool FileFlush(FileHandle *f) { return fflush((FILE *) f) == 0; }
u64 FileWrite(const void *data, u64 size, u64 count, FileHandle *f) { return fwrite(data, size, count, (FILE *) f); }

bool FileSeek(FileHandle *f, s64 offset, FileSeekOrigin origin)
{
	int whence = origin == FileSeekOrigin::Start ? SEEK_SET : origin == FileSeekOrigin::Current ? SEEK_CUR : SEEK_END;
	return fseek((FILE *) f, offset, whence) == 0;
}

u64 FileWriteFormatted(FileHandle *f, const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	int n = vfprintf((FILE *) f, fmt, args);
	va_end(args);
	return n < 0 ? 0 : n;
}

u64 FileLength(FileHandle *f)
{
	long pos = ftell((FILE *) f);
	fseek((FILE *) f, 0, SEEK_END);
	long len = ftell((FILE *) f);
	fseek((FILE *) f, pos, SEEK_SET);
	return len < 0 ? 0 : len;
}

void Log(LogLevel level, const char *fmt, ...)
{
	if (level < Error) {
		return;
	}
	va_list args;
	va_start(args, fmt);
	vfprintf(stderr, fmt, args);
	va_end(args);
}

struct Thread {
	std::thread t;
};
Thread *Thread_Create(std::function<void()> func) { return new Thread{std::thread(std::move(func))}; }
void Thread_Free(Thread *t)
{
	if (t->t.joinable()) {
		t->t.detach();
	}
	delete t;
}
void Thread_Wait(Thread *t) { t->t.join(); }

struct Semaphore {
	std::mutex m;
	std::condition_variable cv;
	int count = 0;
};
Semaphore *Semaphore_Create() { return new Semaphore; }
void Semaphore_Free(Semaphore *s) { delete s; }
void Semaphore_Reset(Semaphore *s)
{
	std::lock_guard<std::mutex> lock(s->m);
	s->count = 0;
}
void Semaphore_Wait(Semaphore *s)
{
	std::unique_lock<std::mutex> lock(s->m);
	s->cv.wait(lock, [s] { return s->count > 0; });
	--s->count;
}
bool Semaphore_TryWait(Semaphore *s, int timeout_ms)
{
	std::unique_lock<std::mutex> lock(s->m);
	if (!s->cv.wait_for(lock, std::chrono::milliseconds(timeout_ms), [s] { return s->count > 0; })) {
		return false;
	}
	--s->count;
	return true;
}
void Semaphore_Post(Semaphore *s, int count)
{
	{
		std::lock_guard<std::mutex> lock(s->m);
		s->count += count;
	}
	s->cv.notify_all();
}

struct Mutex {
	std::mutex m;
};
Mutex *Mutex_Create() { return new Mutex; }
void Mutex_Free(Mutex *m) { delete m; }
void Mutex_Lock(Mutex *m) { m->m.lock(); }
void Mutex_Unlock(Mutex *m) { m->m.unlock(); }
bool Mutex_TryLock(Mutex *m) { return m->m.try_lock(); }

void Sleep(u64 usecs) { std::this_thread::sleep_for(std::chrono::microseconds(usecs)); }

static u64 since_start()
{
	static const auto start = std::chrono::steady_clock::now();
	return std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now() - start).count();
}
u64 GetMSCount() { return since_start() / 1000; }
u64 GetUSCount() { return since_start(); }

// O cartucho chama isto a cada escrita; o arquivo so e gravado no fim.
void WriteNDSSave(const u8 *data, u32 len, u32, u32, void *)
{
	save_data = data;
	save_len = len;
}
void WriteGBASave(const u8 *, u32, u32, u32, void *) {}
void WriteFirmware(const Firmware &, u32, u32, void *) {}
void WriteDateTime(int, int, int, int, int, int, void *) {}

// Sem rede, sem jogo em grupo, sem camera, sem microfone, sem acessorio.
void MP_Begin(void *) {}
void MP_End(void *) {}
int MP_SendPacket(u8 *, int, u64, void *) { return 0; }
int MP_RecvPacket(u8 *, u64 *, void *) { return 0; }
int MP_SendCmd(u8 *, int, u64, void *) { return 0; }
int MP_SendReply(u8 *, int, u64, u16, void *) { return 0; }
int MP_SendAck(u8 *, int, u64, void *) { return 0; }
int MP_RecvHostPacket(u8 *, u64 *, void *) { return 0; }
u16 MP_RecvReplies(u8 *, u64, u16, void *) { return 0; }
int Net_SendPacket(u8 *, int, void *) { return 0; }
int Net_RecvPacket(u8 *, void *) { return 0; }
void Camera_Start(int, void *) {}
void Camera_Stop(int, void *) {}
void Camera_CaptureFrame(int, u32 *, int, int, bool, void *) {}
void Mic_Start(void *) {}
void Mic_Stop(void *) {}
int Mic_ReadInput(s16 *, int, void *) { return 0; }
AACDecoder *AAC_Init() { return nullptr; }
void AAC_DeInit(AACDecoder *) {}
bool AAC_Configure(AACDecoder *, int, int) { return false; }
bool AAC_DecodeFrame(AACDecoder *, const void *, int, void *, int) { return false; }
bool Addon_KeyDown(KeyType, void *) { return false; }
void Addon_RumbleStart(u32, void *) {}
void Addon_RumbleStop(void *) {}
float Addon_MotionQuery(MotionQueryType, void *) { return 0; }
DynamicLibrary *DynamicLibrary_Load(const char *) { return nullptr; }
void DynamicLibrary_Unload(DynamicLibrary *) {}
void *DynamicLibrary_LoadFunction(DynamicLibrary *, const char *) { return nullptr; }

} // namespace melonDS::Platform

// --- o roteiro -------------------------------------------------------------

static const char *KEYS[] = {"A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L", "X", "Y"};

static int key_bit(const char *name, size_t len)
{
	for (int i = 0; i < 12; ++i) {
		if (strlen(KEYS[i]) == len && strncmp(KEYS[i], name, len) == 0) {
			return i;
		}
	}
	return -1;
}

static unsigned long frames = 0;

static void run(NDS &nds, u32 keys, unsigned long n)
{
	nds.SetKeyMask(~keys & 0xFFF); // a mascara e invertida: bit zerado = apertada
	for (unsigned long i = 0; i < n; ++i) {
		nds.RunFrame();
		++frames;
	}
	nds.SetKeyMask(0xFFF);
}

static std::vector<u8> read_file(const char *path)
{
	std::vector<u8> data;
	FILE *f = fopen(path, "rb");
	if (!f) {
		return data;
	}
	fseek(f, 0, SEEK_END);
	data.resize(ftell(f));
	fseek(f, 0, SEEK_SET);
	if (fread(data.data(), 1, data.size(), f) != data.size()) {
		data.clear();
	}
	fclose(f);
	return data;
}

// As duas telas numa imagem so, a de cima em cima, como no aparelho.
static bool shot(NDS &nds, const std::string &path)
{
	const auto &screens = nds.GPU.Framebuffer[nds.GPU.FrontBuffer];
	if (!screens[0] || !screens[1]) {
		return false;
	}
	const size_t screen = 256 * 192;
	std::vector<u32> pixels(screen * 2);
	memcpy(pixels.data(), screens[0].get(), screen * 4);
	memcpy(pixels.data() + screen, screens[1].get(), screen * 4);
	png_image image{};
	image.version = PNG_IMAGE_VERSION;
	image.width = 256;
	image.height = 384;
	image.format = PNG_FORMAT_BGRA; // o nucleo guarda 0xAARRGGBB
	return png_image_write_to_file(&image, path.c_str(), 0, pixels.data(), 0, nullptr) != 0;
}

int main(int argc, char **argv)
{
	if (argc < 3) {
		fprintf(stderr, "uso: roda <jogo.nds> <pasta> [--sav arquivo] passo...\n");
		return 2;
	}
	const char *rom_path = argv[1];
	const std::string dir = argv[2];
	int first = 3;
	const char *sav = nullptr;
	while (first + 1 < argc && strncmp(argv[first], "--", 2) == 0) {
		if (strcmp(argv[first], "--sav") == 0) {
			sav = argv[first + 1];
		} else {
			fprintf(stderr, "opcao desconhecida: %s\n", argv[first]);
			return 2;
		}
		first += 2;
	}

	std::vector<u8> rom = read_file(rom_path);
	if (rom.empty()) {
		fprintf(stderr, "nao abri o jogo: %s\n", rom_path);
		return 1;
	}
	NDSCart::NDSCartArgs cart_args{};
	if (sav) {
		std::vector<u8> data = read_file(sav);
		if (data.empty()) {
			fprintf(stderr, "nao abri o .sav: %s\n", sav);
			return 1;
		}
		cart_args.SRAM = std::make_unique<u8[]>(data.size());
		memcpy(cart_args.SRAM.get(), data.data(), data.size());
		cart_args.SRAMLength = data.size();
	}
	auto cart = NDSCart::ParseROM(rom.data(), rom.size(), nullptr, std::move(cart_args));
	if (!cart) {
		fprintf(stderr, "nao reconheci o jogo: %s\n", rom_path);
		return 1;
	}
	Platform::save_path = dir + "/jogo.sav";

	// BIOS livre, firmware gerado, renderizador por software: os padroes do NDSArgs.
	auto nds = std::make_unique<NDS>(NDSArgs{}, nullptr);
	nds->SetNDSCart(std::move(cart));
	nds->Reset();
	const char *slash = strrchr(rom_path, '/');
	nds->SetupDirectBoot(slash ? slash + 1 : rom_path);
	nds->Start();

	for (int i = first; i < argc; ++i) {
		const char *step = argv[i];
		if (strncmp(step, "shot=", 5) == 0) {
			if (!shot(*nds, dir + "/" + (step + 5) + ".png")) {
				fprintf(stderr, "nao gravei a foto: %s\n", step);
				return 1;
			}
		} else if (strncmp(step, "hold=", 5) == 0) {
			const char *colon = strchr(step + 5, ':');
			int bit = colon ? key_bit(step + 5, colon - (step + 5)) : -1;
			if (bit < 0) {
				fprintf(stderr, "hold invalido: %s\n", step);
				return 2;
			}
			run(*nds, 1u << bit, strtoul(colon + 1, nullptr, 10));
		} else if (strncmp(step, "touch=", 6) == 0) {
			unsigned x, y;
			unsigned long n = 4;
			if (sscanf(step + 6, "%u,%u:%lu", &x, &y, &n) < 2 || x > 255 || y > 191) {
				fprintf(stderr, "touch invalido: %s\n", step);
				return 2;
			}
			nds->TouchScreen(x, y);
			run(*nds, 0, n);
			nds->ReleaseScreen();
			run(*nds, 0, 8);
		} else if (step[0] >= '0' && step[0] <= '9') {
			run(*nds, 0, strtoul(step, nullptr, 10));
		} else {
			const char *star = strchr(step, '*');
			size_t len = star ? (size_t) (star - step) : strlen(step);
			int bit = key_bit(step, len);
			if (bit < 0) {
				fprintf(stderr, "passo desconhecido: %s\n", step);
				return 2;
			}
			unsigned long times = star ? strtoul(star + 1, nullptr, 10) : 1;
			for (unsigned long t = 0; t < times; ++t) {
				run(*nds, 1u << bit, 4);
				run(*nds, 0, 8);
			}
		}
	}

	if (Platform::save_data) {
		FILE *f = fopen(Platform::save_path.c_str(), "wb");
		if (!f || fwrite(Platform::save_data, 1, Platform::save_len, f) != Platform::save_len) {
			fprintf(stderr, "nao gravei o save: %s\n", Platform::save_path.c_str());
			return 1;
		}
		fclose(f);
	}
	printf("quadros: %lu\n", frames);
	return 0;
}
