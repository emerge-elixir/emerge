/* Dependency/load smoke only; this does not initialize a BEAM or GPU runtime. */
#include <dlfcn.h>
#include <stdio.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!library) {
        fprintf(stderr, "%s\n", dlerror());
        return 1;
    }
    if (!dlsym(library, "nif_init")) {
        const char *error = dlerror();
        fprintf(stderr, "%s\n", error ? error : "missing nif_init");
        dlclose(library);
        return 1;
    }
    return dlclose(library) != 0;
}
