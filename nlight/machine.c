/* nlightRT 机器层：裸内存原语。
 *
 * 只有这几个函数必须是机器码级别的——它们是自举的地板：
 * 没有「读写任意地址」就没法在 Light 里表达数据结构。
 * 运行时的其余部分（字符串、数组、字典、打印、文件、数学）全部用 Light 实现，
 * 见 nrt.light。
 */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <math.h>

/* ---------------------------------------------------------------- 裸内存原语
 * 自举的地板：没有「读写任意地址」就没法在 Light 里表达数据结构。
 * 分配时前面留 8 字节存总长度，释放时才知道用哪个 Layout。
 */
/* 命令行原始字节：静态库拿不到 main 的 argv，从 /proc/self/cmdline 读。
 * 返回 malloc 的缓冲区（由 Light 侧复制成字符串后释放）与字节数。 */
int64_t nrt_cmdline(int64_t *out_len)
{
    FILE *f = fopen("/proc/self/cmdline", "rb");
    if (!f) {
        if (out_len) {
            *out_len = 0;
        }
        return 0;
    }
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (size <= 0) {
        fclose(f);
        if (out_len) {
            *out_len = 0;
        }
        return 0;
    }
    char *buf = (char *)malloc((size_t)size);
    size_t got = fread(buf, 1, (size_t)size, f);
    fclose(f);
    if (out_len) {
        *out_len = (int64_t)got;
    }
    return (int64_t)(intptr_t)buf;
}

/* 存活对象计数（编译器/调试器查询，与参考实现同名） */
static int64_t g_live_strings = 0;
static int64_t g_live_arrays = 0;
static int64_t g_live_dicts = 0;

int64_t lightrt_live_strings(void) { return g_live_strings; }
int64_t lightrt_live_arrays(void) { return g_live_arrays; }
int64_t lightrt_live_dicts(void) { return g_live_dicts; }

void nrt_note_string(int64_t delta) { g_live_strings += delta; }
void nrt_note_array(int64_t delta) { g_live_arrays += delta; }
void nrt_note_dict(int64_t delta) { g_live_dicts += delta; }

/* 数组越界：与参考实现一样直接终止，不返回垃圾值 */
void nrt_oob(int64_t index, int64_t len)
{
    fprintf(stderr, "[nlightRT 内存安全错误] 数组越界读取：索引 %lld >= 长度 %lld\n",
            (long long)index, (long long)len);
    fflush(stderr);
    abort();
}

int64_t light_mem_alloc(int64_t n)
{
    if (n <= 0) {
        return 0;
    }
    size_t total = (size_t)n + 8;
    void *p = calloc(1, total);
    if (!p) {
        return 0;
    }
    *(int64_t *)p = (int64_t)total;
    return (int64_t)p + 8;
}

void light_mem_free(int64_t p)
{
    if (p == 0) {
        return;
    }
    void *head = (void *)(p - 8);
    memset(head, 0xAA, 8);   /* 故意写坏头部，方便抓重复释放 */
    free(head);
}

int64_t light_mem_copy(int64_t dst, int64_t src, int64_t n)
{
    if (dst == 0 || src == 0 || n <= 0) {
        return dst;
    }
    memmove((void *)dst, (const void *)src, (size_t)n);
    return dst;
}

void light_mem_zero(int64_t p, int64_t n)
{
    if (p == 0 || n <= 0) {
        return;
    }
    memset((void *)p, 0, (size_t)n);
}

int64_t light_mem_load(int64_t p)
{
    if (p == 0) {
        return 0;
    }
    int64_t v;
    memcpy(&v, (const void *)p, 8);
    return v;
}

void light_mem_store(int64_t p, int64_t v)
{
    if (p == 0) {
        return;
    }
    memcpy((void *)p, &v, 8);
}

int64_t light_mem_load8(int64_t p)
{
    if (p == 0) {
        return 0;
    }
    unsigned char v;
    memcpy(&v, (const void *)p, 1);
    return (int64_t)v;
}

void light_mem_store8(int64_t p, int64_t v)
{
    if (p == 0) {
        return;
    }
    unsigned char b = (unsigned char)(v & 0xff);
    memcpy((void *)p, &b, 1);
}

/* 字符串 -> double 的位模式：IEEE-754 编码定点算法做不出来，交给 libc */
int64_t nrt_strtod(int64_t data, int64_t len)
{
    char buf[64];
    if (data == 0 || len <= 0 || len >= (int64_t)sizeof(buf)) {
        return 0;
    }
    memcpy(buf, (const void *)data, (size_t)len);
    buf[len] = '\0';
    double d = strtod(buf, NULL);
    int64_t bits;
    memcpy(&bits, &d, 8);
    return bits;
}

#include <string.h>
#include <stdio.h>
#include <math.h>
#include <pthread.h>
#include <stdint.h>

/* Light 线程入口：从参数里取出函数指针与实参再调用 */
/* 线程句柄表：join 逐个等待 */
#define NRT_MAX_THREADS 256
static pthread_t g_threads[NRT_MAX_THREADS];
static int g_thread_count = 0;

/* pthread 入口只有一个参数：把 (入口, 实参) 从结构里取出来再调 */
struct nrt_thread_arg { int64_t entry, arg; };

static void *nrt_thread_entry(void *raw)
{
    struct nrt_thread_arg *p = (struct nrt_thread_arg *)raw;
    int64_t (*fn)(int64_t) = (int64_t (*)(int64_t))p->entry;
    fn(p->arg);
    free(p);
    return NULL;
}

int64_t nrt_thread_start(int64_t entry, int64_t arg)
{
    /* 入口是 Light 函数：把 (入口, 实参) 打包成一个结构传进 pthread */
    struct nrt_thread_arg *p = malloc(sizeof(*p));
    if (!p) {
        return 0;
    }
    p->entry = entry;
    p->arg = arg;
    if (g_thread_count >= NRT_MAX_THREADS) {
        free(p);
        return 0;
    }
    if (pthread_create(&g_threads[g_thread_count], NULL, nrt_thread_entry, p) != 0) {
        free(p);
        return 0;
    }
    g_thread_count++;
    return (int64_t)g_thread_count;   /* 句柄就是序号 + 1 */
}

int64_t nrt_thread_join(int64_t handle)
{
    if (handle <= 0 || handle > g_thread_count) {
        return 0;
    }
    pthread_join(g_threads[handle - 1], NULL);
    return 0;
}

int64_t nrt_thread_join_all(void)
{
    for (int i = 0; i < g_thread_count; i++) {
        pthread_join(g_threads[i], NULL);
    }
    g_thread_count = 0;
    return 0;
}

/* double 位模式 -> 文本，写入 buf（至少 40 字节），返回写入的字节数。
 * 浮点格式化交给 libc：定点算法丢精度，且 Light 侧没有 %f。 */
int64_t nrt_ftoa(int64_t bits, int64_t buf)
{
    double v;
    memcpy(&v, &bits, 8);
    char *out = (char *)(intptr_t)buf;
    int n = snprintf(out, 40, "%g", v);
    if (n < 0) {
        n = 0;
    }
    return (int64_t)n;
}

/* 浮点转文本：最短可往返表示（与参考实现的 format!("{}") 一致） */
int64_t nrt_ftoa_rt(int64_t bits, int64_t buf)
{
    double v;
    memcpy(&v, &bits, 8);
    char *out = (char *)(intptr_t)buf;
    for (int prec = 1; prec <= 17; prec++) {
        char tmp[64];
        int n = snprintf(tmp, sizeof(tmp), "%.*g", prec, v);
        if (n < 0) {
            continue;
        }
        double back = strtod(tmp, NULL);
        if (memcmp(&back, &v, 8) == 0) {
            memcpy(out, tmp, (size_t)n + 1);
            return (int64_t)n;
        }
    }
    int n = snprintf(out, 40, "%.17g", v);
    return (int64_t)(n < 0 ? 0 : n);
}

/* 异常：抛出的消息存在这里（Light 没有全局变量，跨调用共享放机器层） */
static int64_t g_error = 0;

void nrt_throw(int64_t msg)
{
    g_error = msg;
}

int64_t nrt_take_error(void)
{
    int64_t e = g_error;
    g_error = 0;
    return e;
}

/* ---------------------------------------------------------------- 数学
 * Light 侧按 i64 位模式传 double，这里做一次转换。 */
#define MATH1(name)                                                     \
    int64_t nrt_math_##name(int64_t bits)                              \
    {                                                                  \
        double x, y;                                                   \
        memcpy(&x, &bits, 8);                                          \
        y = name(x);                                                   \
        int64_t out;                                                   \
        memcpy(&out, &y, 8);                                           \
        return out;                                                    \
    }
MATH1(sin)
MATH1(cos)
MATH1(tan)
MATH1(sqrt)
MATH1(log)
MATH1(log10)
MATH1(floor)
MATH1(ceil)

int64_t nrt_math_pow(int64_t a, int64_t b)
{
    double x, y, z;
    memcpy(&x, &a, 8);
    memcpy(&y, &b, 8);
    z = pow(x, y);
    int64_t out;
    memcpy(&out, &z, 8);
    return out;
}

int64_t nrt_math_pi(void)
{
    double v = 3.14159265358979323846;
    int64_t out;
    memcpy(&out, &v, 8);
    return out;
}

int64_t nrt_math_e(void)
{
    double v = 2.71828182845904523536;
    int64_t out;
    memcpy(&out, &v, 8);
    return out;
}

int64_t nrt_math_random(void)
{
    double v = (double)rand() / (double)RAND_MAX;
    int64_t out;
    memcpy(&out, &v, 8);
    return out;
}

int64_t nrt_math_random_int(int64_t lo, int64_t hi)
{
    if (hi <= lo) {
        return lo;
    }
    return lo + rand() % (hi - lo);
}

int64_t nrt_floor_div(int64_t a, int64_t b)
{
    if (b == 0) {
        return 0;
    }
    int64_t q = a / b;
    int64_t r = a % b;
    if (r != 0 && ((r < 0) != (b < 0))) {
        q -= 1;
    }
    return q;
}

int64_t nrt_pow_int(int64_t base, int64_t exp)
{
    int64_t result = 1;
    if (exp < 0) {
        return 0;
    }
    for (int64_t i = 0; i < exp; i++) {
        result *= base;
    }
    return result;
}

void nrt_exit(int64_t code)
{
    exit((int)code);
}
