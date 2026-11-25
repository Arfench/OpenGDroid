/* Verifies renderer selection and GMA parsing through the C ABI on a device. */
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef char *(*renderer_launch_fn)(const char *, int, int);
typedef int (*renderer_clear_fn)(const char *);
typedef int64_t (*gma_count_fn)(const uint8_t *, size_t);
typedef char *(*gma_info_fn)(const uint8_t *, size_t);
typedef int (*gma_path_allowed_fn)(const char *);
typedef void (*free_fn)(char *);

static int failures = 0;

static void check(int cond, const char *what) {
    printf("  [%s] %s\n", cond ? "PASS" : "FAIL", what);
    if (!cond) failures++;
}

/* Builds a minimal RAW GMA in memory. Returns malloc'd buffer. */
static uint8_t *build_gma(size_t *out_len, int with_bad_path) {
    size_t cap = 4096;
    uint8_t *b = calloc(1, cap);
    size_t p = 0;

#define PUT_U32BE(v)                     \
    do {                                 \
        uint32_t _v = (uint32_t)(v);     \
        b[p++] = (_v >> 24) & 0xFF;      \
        b[p++] = (_v >> 16) & 0xFF;      \
        b[p++] = (_v >> 8) & 0xFF;       \
        b[p++] = _v & 0xFF;              \
    } while (0)

#define PUT_I64BE(v)                     \
    do {                                 \
        uint64_t _v = (uint64_t)(v);     \
        for (int _i = 7; _i >= 0; _i--) b[p++] = (_v >> (_i * 8)) & 0xFF; \
    } while (0)

#define PUT_STR(s)                                  \
    do {                                            \
        const char *_s = (s);                       \
        memcpy(b + p, _s, strlen(_s));              \
        p += strlen(_s);                            \
        b[p++] = 0;                                 \
    } while (0)

    PUT_U32BE(1145130311u); /* magic GMA7 */
    b[p++] = 1;             /* version */
    PUT_I64BE(76561197960265728LL); /* steamid */
    PUT_I64BE(1700000000LL);        /* timestamp */
    PUT_STR("Test Addon");
    PUT_STR("desc");
    PUT_STR("tester");
    PUT_U32BE(7); /* addon version */

    /* file 1: allowed */
    PUT_U32BE(1);
    PUT_STR("lua/autorun/init.lua");
    PUT_I64BE(4);
    PUT_U32BE(0xDEADBEEF);
    /* file 2: rejected (engine library) when requested */
    if (with_bad_path) {
        PUT_U32BE(2);
        PUT_STR("bin/linux64/libengine.so");
        PUT_I64BE(3);
        PUT_U32BE(0x12345678);
    }
    PUT_U32BE(0); /* end of index */

    memcpy(b + p, "abcd", 4);
    p += 4;
    if (with_bad_path) {
        memcpy(b + p, "xyz", 3);
        p += 3;
    }
    PUT_U32BE(0xCAFEF00D); /* addon crc */

#undef PUT_U32BE
#undef PUT_I64BE
#undef PUT_STR

    *out_len = p;
    return b;
}

int main(void) {
    void *h = dlopen("libopengdroid.so", RTLD_NOW);
    if (!h) {
        printf("[FAIL] dlopen: %s\n", dlerror());
        return 1;
    }

    renderer_launch_fn r_launch = (renderer_launch_fn)dlsym(h, "opengdroid_renderer_launch");
    renderer_clear_fn r_clear = (renderer_clear_fn)dlsym(h, "opengdroid_renderer_clear");
    gma_count_fn g_count = (gma_count_fn)dlsym(h, "opengdroid_gma_count");
    gma_info_fn g_info = (gma_info_fn)dlsym(h, "opengdroid_gma_info");
    gma_path_allowed_fn g_allowed = (gma_path_allowed_fn)dlsym(h, "opengdroid_gma_path_allowed");
    free_fn str_free = (free_fn)dlsym(h, "opengdroid_string_free");

    check(r_launch && r_clear && g_count && g_info && g_allowed && str_free,
          "new symbols resolved");
    if (failures) return 1;

    printf("== renderer: not requested ==\n");
    char *j = r_launch("/data/local/tmp/rdir", 0, 4198400);
    printf("  %s\n", j ? j : "(null)");
    check(j && strstr(j, "\"vulkan\":false"), "GLES when not requested");
    if (j) str_free(j);

    printf("== renderer: requested, device supports 1.1 ==\n");
    j = r_launch("/data/local/tmp/rdir", 1, 4198400);
    printf("  %s\n", j ? j : "(null)");
    check(j && strstr(j, "\"vulkan\":true"), "Vulkan accepted on capable device");
    if (j) str_free(j);

    printf("== renderer: second launch sees pending marker -> GLES ==\n");
    j = r_launch("/data/local/tmp/rdir", 1, 4198400);
    printf("  %s\n", j ? j : "(null)");
    check(j && strstr(j, "\"vulkan\":false"), "pending marker forces GLES");
    check(j && strstr(j, "closed before its first frame"), "notice explains the crash");
    if (j) str_free(j);

    printf("== renderer: clear marker allows retry ==\n");
    check(r_clear("/data/local/tmp/rdir") == 0, "clear returns 0");
    j = r_launch("/data/local/tmp/rdir", 1, 4198400);
    check(j && strstr(j, "\"vulkan\":true"), "Vulkan accepted after clear");
    if (j) str_free(j);
    r_clear("/data/local/tmp/rdir");

    printf("== renderer: old device declined ==\n");
    j = r_launch("/data/local/tmp/rdir", 1, 4000000);
    printf("  %s\n", j ? j : "(null)");
    check(j && strstr(j, "\"vulkan\":false"), "old device gets GLES");
    check(j && strstr(j, "does not report Vulkan 1.1"), "reason recorded");
    if (j) str_free(j);
    r_clear("/data/local/tmp/rdir");

    printf("== gma: parse ==\n");
    size_t len = 0;
    uint8_t *gma = build_gma(&len, 0);
    printf("  built %zu bytes\n", len);
    check(g_count(gma, len) == 1, "one file counted");

    char *info = g_info(gma, len);
    printf("  %s\n", info ? info : "(null)");
    check(info && strstr(info, "\"title\":\"Test Addon\""), "title parsed");
    check(info && strstr(info, "\"files\":1"), "file count reported");
    check(info && strstr(info, "\"allowed\":1"), "allowed count reported");
    if (info) str_free(info);
    free(gma);

    printf("== gma: with a rejected path ==\n");
    gma = build_gma(&len, 1);
    check(g_count(gma, len) == 2, "two files counted");
    info = g_info(gma, len);
    printf("  %s\n", info ? info : "(null)");
    check(info && strstr(info, "\"rejected\":1"), "engine library rejected");
    if (info) str_free(info);
    free(gma);

    printf("== gma: whitelist ==\n");
    check(g_allowed("lua/autorun/x.lua") == 1, "lua allowed");
    check(g_allowed("maps/gm_construct.bsp") == 1, "map allowed");
    check(g_allowed("bin/libengine.so") == 0, "engine library rejected");
    check(g_allowed("../../etc/passwd") == 0, "traversal rejected");
    check(g_allowed(NULL) == -1, "null returns -1");

    printf("== gma: non-GMA input ==\n");
    static const uint8_t junk[] = "not an addon at all";
    check(g_count(junk, sizeof(junk) - 1) == -1, "garbage rejected");

    dlclose(h);
    printf("\n%s\n", failures ? "RESULT: FAILURES" : "RESULT: ALL PASS");
    return failures ? 1 : 0;
}
