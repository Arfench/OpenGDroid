/* Smoke test for libopengdroid.so on a real Android device. */
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef const char *(*version_fn)(void);
typedef char *(*shared_root_fn)(const char *);
typedef char *(*build_args_fn)(const char *, uint32_t, uint32_t, _Bool, uint32_t,
                               uint32_t, const char *);
typedef int64_t (*loc_count_fn)(const uint8_t *, size_t);
typedef char *(*loc_lookup_fn)(const uint8_t *, size_t, const char *);
typedef void (*free_fn)(char *);

static int failures = 0;

static void check(int cond, const char *what) {
    printf("  [%s] %s\n", cond ? "PASS" : "FAIL", what);
    if (!cond) failures++;
}

int main(void) {
    printf("== dlopen ==\n");
    void *h = dlopen("libopengdroid.so", RTLD_NOW);
    if (!h) {
        printf("  [FAIL] dlopen: %s\n", dlerror());
        return 1;
    }
    printf("  [PASS] dlopen\n");

    version_fn ver = (version_fn)dlsym(h, "opengdroid_version");
    shared_root_fn shared_root = (shared_root_fn)dlsym(h, "opengdroid_shared_root");
    build_args_fn build_args = (build_args_fn)dlsym(h, "opengdroid_build_args");
    loc_count_fn loc_count = (loc_count_fn)dlsym(h, "opengdroid_localization_count");
    loc_lookup_fn loc_lookup = (loc_lookup_fn)dlsym(h, "opengdroid_localization_lookup");
    free_fn str_free = (free_fn)dlsym(h, "opengdroid_string_free");

    check(ver && shared_root && build_args && loc_count && loc_lookup && str_free,
          "all 6 symbols resolved");
    if (failures) return 1;

    printf("== version ==\n");
    const char *v = ver();
    printf("  version = %s\n", v);
    check(v && strlen(v) > 0, "version non-empty");

    printf("== shared_root ==\n");
    char *root = shared_root("/storage/emulated/0");
    printf("  root = %s\n", root ? root : "(null)");
    check(root && strstr(root, "srceng") != NULL, "root ends with srceng");
    str_free(root);

    printf("== build_args (vulkan + map) ==\n");
    char *args = build_args("garrysmod", 1520, 720, 1, 512, 1, "+map gm_construct");
    printf("  args = %s\n", args ? args : "(null)");
    check(args && strstr(args, "-game garrysmod"), "has -game garrysmod");
    check(args && strstr(args, "-sourcevr_native_vulkan"), "has vulkan token");
    check(args && strstr(args, "+map gm_construct"), "has +map");
    check(args && strstr(args, "-w 1520") && strstr(args, "-h 720"), "has resolution");
    str_free(args);

    printf("== build_args (gles) ==\n");
    char *gles = build_args("garrysmod", 1280, 720, 0, 512, 0, "");
    check(gles && strstr(gles, "-sourcevr_native_vulkan") == NULL, "no vulkan token for GLES");
    check(gles && strstr(gles, "-fullscreen") == NULL, "no -fullscreen when windowed");
    str_free(gles);

    printf("== localization: UTF-16LE (the case the original launcher breaks on) ==\n");
    /* "A" "Alpha" in UTF-16LE with BOM */
    static const uint8_t u16[] = {
        0xFF, 0xFE,
        '"', 0x00, 'A', 0x00, '"', 0x00, ' ', 0x00,
        '"', 0x00, 'A', 0x00, 'l', 0x00, 'p', 0x00, 'h', 0x00, 'a', 0x00, '"', 0x00,
        0x0A, 0x00,
    };
    int64_t n = loc_count(u16, sizeof(u16));
    printf("  entries = %lld\n", (long long)n);
    check(n == 1, "one entry parsed from UTF-16LE");

    char *val = loc_lookup(u16, sizeof(u16), "A");
    printf("  lookup(A) = %s\n", val ? val : "(null)");
    check(val && strcmp(val, "Alpha") == 0, "lookup returns Alpha");
    str_free(val);

    printf("== localization: plain UTF-8 ==\n");
    static const uint8_t u8[] = "\"K\" \"V\"\n";
    check(loc_count(u8, sizeof(u8) - 1) == 1, "one entry from UTF-8");

    printf("\n%s\n", failures ? "RESULT: FAILURES" : "RESULT: ALL PASS");
    dlclose(h);
    return failures ? 1 : 0;
}
