# Builds OpenSSL 3.5.9 into /kairo/package (packages/openssl/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# openssl-3.5.9.
. ./recipe.sh "$1"
NAME=openssl
VERSION=3.5.9
CHILD=openssl-3.5.9

# OpenSSL's Configure is its own, in perl, and takes its target by
# name, not by triple. Its directory for configuration and certificates
# is /etc/ssl, where ca-certificates puts its bundle; its libraries,
# engines, and providers go in the library directory (§60). Without
# its static libraries and its documentation. The time it records is
# SOURCE_DATE_EPOCH's, which Kairo sets.
case "$TRIPLE" in
i686-*) TARGET=linux-x86 ;;
x86_64-*) TARGET=linux-x86_64 ;;
esac
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
perl Configure "$TARGET" --prefix=/usr --libdir="$LIBDIR" --openssldir=/etc/ssl \
    shared no-docs no-tests
make -j"$JOBS"
make install_sw install_ssldirs DESTDIR="$OUT/$CHILD"
find "$OUT/$CHILD" -name '*.a' -delete

finish

# It works, installed over what it was built with.
install_over
openssl version | grep -q "^OpenSSL 3.5.9 "
[ "$(printf made | openssl dgst -sha256 -r | cut -d" " -f1)" = "$(printf made | sha256sum | cut -d" " -f1)" ]
# A key, a certificate it signs for itself, and the certificate checked
# against itself.
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
    -keyout key.pem -out cert.pem -subj /CN=made -days 1
openssl verify -CAfile cert.pem cert.pem
