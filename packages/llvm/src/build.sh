# Builds LLVM into /kairo/package (packages/llvm/README.md) for the
# target named as the first argument, x86-64 alone (DECISIONS.md §65),
# with packages alone (data:build-base, §61): two children, llvm-<v>,
# all of it under /usr/lib/llvm-<major>/, and libllvm-<v>, its shared
# library in the library directory.
. ./recipe.sh "$1"
NAME=llvm
VERSION=21.1.8
MAJOR=${VERSION%%.*}
MINOR=${VERSION#*.}
MINOR=${MINOR%%.*}
CHILD=llvm-$VERSION
PREFIX=/usr/lib/llvm-$MAJOR

# LLVM itself, of the project's release, built by CMake with Ninja: its
# code generator for x86 alone, which is what is built here, more when
# something asks for them; one shared library, libLLVM, that its tools
# and whatever builds against it link, its component libraries
# installed too, for CMake's find_package(LLVM); zlib and zstd for
# compressed debug sections. Under its own directory, named by its
# major, so that two majors sit side by side, as Debian has them; its
# tools find libLLVM beside them, in $ORIGIN/../lib. Without its tests,
# examples, benchmarks, or documentation, and with two links at once,
# as each takes several gigabytes.
unpack "llvm-project-$VERSION.src"
cmake -S "$BUILD/llvm-project-$VERSION.src/llvm" -B "$BUILD/llvm-build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DLLVM_HOST_TRIPLE="$TRIPLE" -DLLVM_DEFAULT_TARGET_TRIPLE="$TRIPLE" \
    -DLLVM_TARGETS_TO_BUILD=X86 \
    -DLLVM_BUILD_LLVM_DYLIB=ON -DLLVM_LINK_LLVM_DYLIB=ON -DLLVM_ENABLE_RTTI=ON \
    -DLLVM_ENABLE_ZLIB=FORCE_ON -DLLVM_ENABLE_ZSTD=FORCE_ON \
    -DLLVM_ENABLE_LIBXML2=OFF -DLLVM_ENABLE_LIBEDIT=OFF -DLLVM_ENABLE_LIBPFM=OFF \
    -DLLVM_ENABLE_FFI=OFF -DLLVM_ENABLE_CURL=OFF -DLLVM_ENABLE_HTTPLIB=OFF \
    -DLLVM_ENABLE_ASSERTIONS=OFF -DLLVM_INCLUDE_TESTS=OFF -DLLVM_BUILD_TESTS=OFF \
    -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF -DLLVM_INCLUDE_DOCS=OFF \
    -DLLVM_PARALLEL_LINK_JOBS=2
cmake --build "$BUILD/llvm-build"
DESTDIR="$OUT/$CHILD" cmake --install "$BUILD/llvm-build"

# Its shared library, copied out to the library directory, where the
# loader finds it for a program built against it, as a child of its
# own (§56, §60): its soname names the release's major and minor, so
# each release's sits beside the others'.
SONAME=libLLVM.so.$MAJOR.$MINOR
[ -f "$OUT/$CHILD$PREFIX/lib/$SONAME" ] || { echo "build.sh: no $SONAME" >&2; exit 1; }
mkdir -p "$OUT/libllvm-$VERSION$LIBDIR"
cp "$OUT/$CHILD$PREFIX/lib/$SONAME" "$OUT/libllvm-$VERSION$LIBDIR/"

finish

# It works, installed over what it was built with: llvm-config says
# what it is, llc compiles a program's IR to an object gcc links and
# that runs, and a program built against its C API, loading libLLVM
# from the library directory, reports its version.
install_over
CONFIG=$PREFIX/bin/llvm-config
[ "$("$CONFIG" --version)" = "$VERSION" ] || { echo "build.sh: llvm-config says $("$CONFIG" --version)" >&2; exit 1; }
[ "$("$CONFIG" --shared-mode)" = shared ] || { echo "build.sh: LLVM isn't shared" >&2; exit 1; }
[ "$("$CONFIG" --targets-built)" = X86 ] || { echo "build.sh: LLVM targets $("$CONFIG" --targets-built)" >&2; exit 1; }
cat > made.ll <<'IR'
@made = private constant [5 x i8] c"made\00"
declare i32 @puts(ptr)
define i32 @main() {
  call i32 @puts(ptr @made)
  ret i32 0
}
IR
"$PREFIX/bin/llc" -filetype=obj -relocation-model=pic -o made.o made.ll
gcc -o made made.o
[ "$(./made)" = made ]
cat > lcheck.c <<'C'
#include <llvm-c/Core.h>
int main(void) {
    unsigned major, minor, patch;
    LLVMGetVersion(&major, &minor, &patch);
    LLVMModuleRef module = LLVMModuleCreateWithName("made");
    LLVMDisposeModule(module);
    return !(major == LLVM_MAJOR && minor == LLVM_MINOR);
}
C
gcc $("$CONFIG" --cflags) -DLLVM_MAJOR="$MAJOR" -DLLVM_MINOR="$MINOR" -o lcheck lcheck.c \
    -L"$LIBDIR" -l:"$SONAME"
./lcheck
