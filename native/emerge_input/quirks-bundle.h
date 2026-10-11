/* SPDX-License-Identifier: Apache-2.0
 * Adapt upstream's FILE-based parser to immutable compiled-in data. */
#include "emerge-quirks.h"

static inline bool parse_file(struct quirks_context *ctx, const char *path);

static FILE *emerge_quirks_open(const char *path) {
    if (strncmp(path, "emerge:", 7) != 0)
        return fopen(path, "r");
    for (size_t i = 0; i < sizeof(emerge_quirks) / sizeof(emerge_quirks[0]); i++) {
        if (strcmp(path + 7, emerge_quirks[i].name) == 0) {
            /* Read-only fmemopen does not modify the static backing buffer. */
            return fmemopen((void *)emerge_quirks[i].data,
                           strlen(emerge_quirks[i].data), "r");
        }
    }
    errno = ENOENT;
    return NULL;
}

static bool emerge_parse_quirks(struct quirks_context *ctx) {
    for (size_t i = 0; i < sizeof(emerge_quirks) / sizeof(emerge_quirks[0]); i++) {
        char path[PATH_MAX];
        snprintf(path, sizeof(path), "emerge:%s", emerge_quirks[i].name);
        if (!parse_file(ctx, path)) return false;
    }
    return true;
}
