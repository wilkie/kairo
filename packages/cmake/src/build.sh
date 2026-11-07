# Builds CMake 4.4.5 into /kairo/package (packages/cmake/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# cmake-4.4.5.
. ./recipe.sh "$1"
NAME=cmake
VERSION=4.4.5
CHILD=cmake-4.4.5

# Bootstrapped with g++ and make, with the libraries it bundles (curl,
# libarchive, zlib, and the rest), as its own releases are, and
# OpenSSL's for curl's TLS. Without its graphical interface or its
# tests.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
CC=gcc CXX=g++ ./bootstrap --prefix=/usr --parallel="$JOBS" --no-qt-gui -- \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_USE_OPENSSL=ON -DBUILD_TESTING=OFF
make -j"$JOBS"
make install DESTDIR="$OUT/$CHILD"

finish

# It works, installed over what it was built with.
install_over
cmake --version | head -1 | grep -q "^cmake version 4.4.5$"
# A project that finds OpenSSL, as CMake finds a package's library in
# the library directory, and runs.
mkdir cmcheck
cd cmcheck
cat > CMakeLists.txt <<'CM'
cmake_minimum_required(VERSION 3.10)
project(made C)
find_package(OpenSSL REQUIRED)
message(STATUS "crypto at ${OPENSSL_CRYPTO_LIBRARY}")
add_executable(made made.c)
target_link_libraries(made OpenSSL::Crypto)
CM
cat > made.c <<'C'
#include <openssl/crypto.h>
int main(void) { return (OpenSSL_version_num() >> 20) != 0x305; }
C
cmake -S . -B build | tee configure.out
grep -q "crypto at $LIBDIR/libcrypto.so" configure.out
cmake --build build
./build/made
