# Builds Mozilla's certificate authorities 3.119.1 into /kairo/package (packages/ca-certificates/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# ca-certificates-3.119.1.
. ./recipe.sh "$1"
NAME=ca-certificates
VERSION=3.119.1
CHILD=ca-certificates-3.119.1

# NSS's certdata.txt, the certificate authorities and what each is
# trusted for, as curl's mk-ca-bundle.pl makes it a bundle of those
# trusted to identify servers, as Debian and Alpine have it, from the
# copy given (-n). Its header gives the data's date, the file's time:
# that of the NSS 3.119.1 release's commit, 2025-12-10. The bundle is
# where Debian and Alpine put it; /etc/ssl/cert.pem, where OpenSSL looks
# with its directory /etc/ssl, links to it.
tar -C "$BUILD" --no-same-owner -xf "$SRC/distfiles/curl-8.17.0.tar.xz" curl-8.17.0/scripts/mk-ca-bundle.pl
cd "$BUILD"
cp "$SRC/distfiles/certdata.txt" .
touch -d @1765406732 certdata.txt
perl curl-8.17.0/scripts/mk-ca-bundle.pl -n -q ca-bundle.crt
mkdir -p "$OUT/$CHILD/etc/ssl/certs"
install -m 644 ca-bundle.crt "$OUT/$CHILD/etc/ssl/certs/ca-certificates.crt"
ln -s certs/ca-certificates.crt "$OUT/$CHILD/etc/ssl/cert.pem"

finish

# It works, installed over what it was built with.
install_over
# As many certificates as NSS trusts to identify servers, each one
# whole.
count=$(grep -c "^-----BEGIN CERTIFICATE-----$" /etc/ssl/cert.pem)
[ "$count" -gt 100 ] && [ "$count" = "$(grep -c "^-----END CERTIFICATE-----$" /etc/ssl/cert.pem)" ]
grep -q "^## Certificate data from Mozilla as of: Wed Dec 10 22:45:32 2025 GMT$" /etc/ssl/cert.pem
