# Builds libffi 3.5.2 into /kairo/package (packages/libffi/README.md) for the
# target named as the first argument, its first release, with packages
# alone (data:build-base, DECISIONS.md §61): one child, libffi-3.5.2.
. ./recipe.sh "$1"
NAME=libffi
VERSION=3.5.2
CHILD=libffi-3.5.2

gnu "$CHILD" "$NAME-$VERSION" --disable-docs

finish

# It works, installed over what it was built with.
install_over
cat > fficheck.c <<'C'
#include <ffi.h>
static int add(int a, int b) { return a + b; }
int main(void) {
    ffi_cif cif;
    ffi_type *args[2] = { &ffi_type_sint, &ffi_type_sint };
    int a = 2, b = 3;
    void *values[2] = { &a, &b };
    ffi_arg result;
    if (ffi_prep_cif(&cif, FFI_DEFAULT_ABI, 2, &ffi_type_sint, args) != FFI_OK) return 1;
    ffi_call(&cif, FFI_FN(add), &result, values);
    return result != 5;
}
C
gcc -o fficheck fficheck.c -lffi
./fficheck
