# Builds curl 8.23.0 into /kairo/package (packages/curl/README.md) for
# the target named as the first argument, its first release, again,
# with every library it uses a package, with packages alone (data:build-base, DECISIONS.md §61): one child,
# curl-8.23.0.
. ./recipe.sh "$1"
NAME=curl
VERSION=8.23.0
CHILD=curl-8.23.0

# TLS by OpenSSL, trusting the bundle ca-certificates puts where Debian
# and Alpine have it; HTTP/2 by nghttp2; the public suffix list, for
# cookies, by libpsl; international domain names by libidn2; and
# compression by zlib, zstd, and brotli; SCP and SFTP by libssh2. Left
# out for now, as no package has it yet: LDAP. Without its static
# library.
gnu "$CHILD" "$NAME-$VERSION" --disable-static --with-openssl --with-zlib --with-zstd \
    --with-ca-bundle=/etc/ssl/certs/ca-certificates.crt --without-ca-path \
    --with-libpsl --with-nghttp2 --with-libidn2 --with-brotli \
    --with-libssh2 --disable-ldap

finish

# It works, installed over what it was built with.
install_over
version=$(curl --version)
for part in "^curl 8\.23\.0 " " libcurl/8\.23\.0 " " OpenSSL/3\.5\.9 " " zlib/1\.3\.1 " " brotli/1\.2\.1 " \
    " zstd/1\.5\.7 " " libidn2/2\.3\.8 " " libpsl/0\.23\.3 " " nghttp2/1\.71\.0" " libssh2/1\.11\.1" "^Protocols: .* scp .*sftp" \
    "^Features: .* HTTP2 " "^Features: .* IDN " "^Features: .* PSL " "^Features: .* brotli " "^Features: .* zstd"; do
    grep -q -- "$part" <<<"$version" || { echo "build.sh: curl lacks $part: $version" >&2; false; }
done
[ "$(curl-config --ca)" = /etc/ssl/certs/ca-certificates.crt ]
echo made >made.txt
[ "$(curl -s "file://$PWD/made.txt")" = made ]
# A request over TLS, to openssl's own server on the loopback, its
# certificate made here and trusted for this alone.
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
    -keyout key.pem -out cert.pem -subj /CN=localhost -addext subjectAltName=DNS:localhost -days 1
openssl s_server -accept 4433 -cert cert.pem -key key.pem -www -quiet &
server=$!
for _ in $(seq 50); do
    curl -s --cacert cert.pem https://localhost:4433/ >tls.out && break
    sleep 0.2
done
kill "$server"
grep -q "s_server" tls.out
