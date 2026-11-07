# Builds perl 5.42.0 into /kairo/package (packages/perl/README.md) for the
# target named as the first argument, its first release, with packages
# alone (data:build-base, DECISIONS.md §61): one child, perl-5.42.0.
. ./recipe.sh "$1"
NAME=perl
VERSION=5.42.0
CHILD=perl-5.42.0

# Configure records the machine's kernel version and the time it ran,
# and any perl already installed; all are fixed here, so the package
# doesn't depend on them. The time, in a setting and in config.sh's
# header, and the installed perl it sets whatever it is given, so they
# are changed in config.sh after, and the files made from config.sh
# made again (Configure -S). Its architecture is the target's, not the
# kernel's, which for x86 is 64-bit.
# The libraries it looks for are in the library directory (§60).
# It finds functions, eaccess among them, that musl declares only for
# _GNU_SOURCE, as Alpine's build defines. With no man directories in
# the environment, it would install its manuals at the artifact's top;
# they go where the rest of the documentation does, and are removed
# with it.
case "$TRIPLE" in
i686-*) ARCH=i686 ;;
x86_64-*) ARCH=x86_64 ;;
esac
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
sh Configure -des -Dprefix=/usr -Dvendorprefix=/usr -Duseshrplib \
    -Dcc=gcc -Accflags=-D_GNU_SOURCE -Dlibpth="$LIBDIR" \
    -Darchname="$ARCH-linux" \
    -Dman1dir=/usr/share/man/man1 -Dman3dir=/usr/share/man/man3 \
    -Dman1ext=1 -Dman3ext=3pm \
    -Dmyhostname=kairo -Dmydomain=.invalid -Dcf_by=kairo \
    -Dcf_email=root@kairo.invalid -Dperladmin=root@kairo.invalid \
    -Dosvers=4.14 -Dmyuname="linux kairo 4.14 $ARCH"
sed -i -e "s/^cf_time=.*/cf_time='Tue Jan  1 00:00:00 UTC 1980'/" \
    -e "s/^# Configuration time: .*/# Configuration time: Tue Jan  1 00:00:00 UTC 1980/" \
    -e "s/^perl5=.*/perl5=''/" config.sh
sh Configure -S
make -j"$JOBS"
make install DESTDIR="$OUT/$CHILD"

finish

# It works, installed over what it was built with.
install_over
[ "$(perl -e 'print "made"')" = made ]
perl -MPOSIX -e 'exit 0'
perl -e 'exit($] == 5.042000 ? 0 : 1)'
