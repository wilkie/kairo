# Builds the Linux kernel's headers 4.14.336 into /kairo/package
# (packages/linux-headers/README.md) for the target named as the first
# argument, a round of the base after the first, with the round before
# it (DECISIONS.md §64): one child, linux-headers-4.14.336.
. ./recipe.sh "$1"
NAME=linux-headers
VERSION=4.14.336
CHILD=linux-headers-4.14.336

# The kernel's headers for the target. Its host helpers predate C23,
# gcc 15's default.
unpack "linux-$VERSION"
cd "$BUILD/linux-$VERSION"
make ARCH="$KERNEL_ARCH" HOSTCC="gcc -std=gnu11" INSTALL_HDR_PATH="$OUT/$CHILD/usr" headers_install

finish

# It works, installed over what it was built with.
install_over
# The version code keeps the sublevel in eight bits, which 4.14's
# Makefile clamps at 255.
printf "#include <linux/version.h>\nint main(void) { return LINUX_VERSION_CODE == KERNEL_VERSION(4, 14, 255) ? 0 : 1; }\n" >v.c
gcc -o v v.c
./v
