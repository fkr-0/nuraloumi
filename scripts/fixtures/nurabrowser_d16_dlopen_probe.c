/* Strict ARMv7/musl runtime closure probe; never exercises a GPU or device.
 * Compile in the pinned Alpine ARMv7 toolchain, then run through the
 * private-root musl loader inside an isolated chroot (network disabled).
 */
#include <dlfcn.h>
#include <stdio.h>

int main(int argc, char **argv)
{
    int failed = 0;

    if (argc < 2) {
        fputs("usage: d16-dlopen-probe /absolute/library.so [...]\n", stderr);
        return 64;
    }
    for (int i = 1; i < argc; ++i) {
        dlerror();
        void *library = dlopen(argv[i], RTLD_NOW | RTLD_LOCAL);
        if (!library) {
            fprintf(stderr, "DLOPEN_FAIL %s: %s\n", argv[i], dlerror());
            ++failed;
            continue;
        }
        printf("DLOPEN_PASS %s\n", argv[i]);
        if (dlclose(library) != 0) {
            fprintf(stderr, "DLCLOSE_FAIL %s: %s\n", argv[i], dlerror());
            ++failed;
        }
    }
    return failed ? 1 : 0;
}
