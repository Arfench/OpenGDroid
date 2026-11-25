/* End-to-end check: parse the real localization file the original launcher fails on. */
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef int64_t (*loc_count_fn)(const uint8_t *, size_t);
typedef char *(*loc_lookup_fn)(const uint8_t *, size_t, const char *);
typedef void (*free_fn)(char *);

static int failures = 0;

static void check(int cond, const char *what) {
    printf("  [%s] %s\n", cond ? "PASS" : "FAIL", what);
    if (!cond) failures++;
}

static uint8_t *slurp(const char *path, size_t *out_len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (size <= 0) {
        fclose(f);
        return NULL;
    }
    uint8_t *buf = malloc((size_t)size);
    if (!buf) {
        fclose(f);
        return NULL;
    }
    if (fread(buf, 1, (size_t)size, f) != (size_t)size) {
        free(buf);
        fclose(f);
        return NULL;
    }
    fclose(f);
    *out_len = (size_t)size;
    return buf;
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "/sdcard/Download/Garry's Mod/sourceengine/resource/valve_english.txt";

    void *h = dlopen("libopengdroid.so", RTLD_NOW);
    if (!h) {
        printf("[FAIL] dlopen: %s\n", dlerror());
        return 1;
    }
    loc_count_fn loc_count = (loc_count_fn)dlsym(h, "opengdroid_localization_count");
    loc_lookup_fn loc_lookup = (loc_lookup_fn)dlsym(h, "opengdroid_localization_lookup");
    free_fn str_free = (free_fn)dlsym(h, "opengdroid_string_free");
    if (!loc_count || !loc_lookup || !str_free) {
        printf("[FAIL] symbols missing\n");
        return 1;
    }

    printf("== file ==\n  %s\n", path);
    size_t len = 0;
    uint8_t *data = slurp(path, &len);
    if (!data) {
        printf("  [SKIP] cannot read file\n");
        return 0;
    }
    printf("  bytes = %zu\n", len);

    printf("== BOM ==\n");
    printf("  first bytes = %02X %02X\n", data[0], data[1]);
    check(data[0] == 0xFF && data[1] == 0xFE, "file is UTF-16LE (BOM FF FE)");

    printf("== parse (the original launcher reports ZERO strings here) ==\n");
    int64_t n = loc_count(data, len);
    printf("  entries = %lld\n", (long long)n);
    check(n > 100, "parser extracts a substantial number of entries");

    printf("== lookups ==\n");
    static const char *keys[] = {"Language", "Valve_Movement_Title", "Valve_Move_Back"};
    for (size_t i = 0; i < sizeof(keys) / sizeof(keys[0]); i++) {
        char *v = loc_lookup(data, len, keys[i]);
        printf("  %-24s -> %s\n", keys[i], v ? v : "(absent)");
        if (strcmp(keys[i], "Language") == 0) {
            /* "Language" is the table header, intentionally skipped by the parser. */
            check(v == NULL, "header key Language is not an entry");
        } else {
            check(v != NULL, "value resolved");
        }
        if (v) str_free(v);
    }

    free(data);
    dlclose(h);
    printf("\n%s\n", failures ? "RESULT: FAILURES" : "RESULT: ALL PASS");
    return failures ? 1 : 0;
}
