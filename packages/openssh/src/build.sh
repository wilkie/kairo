# Builds OpenSSH 10.6p2 into /kairo/package (packages/openssh/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# openssh-10.6p2.
. ./recipe.sh "$1"
NAME=openssh
VERSION=10.6p2
CHILD=openssh-10.6p2

# Its configuration in /etc/ssh, its helpers in /usr/libexec, and its
# server's empty directory for privilege separation where Fedora has it,
# /usr/share/empty.sshd, as a child holds only usr/ and etc/: being
# empty, it isn't in the artifact, and whatever runs sshd makes it, as
# whatever runs it makes the host's keys and the sshd user. Without PAM,
# Kerberos, security keys, or line editing in sftp, for which no
# package is built. Its tag and its child's version are 10.6.2, as a
# version is semver; the child's name keeps OpenSSH's, 10.6p2.
gnu "$CHILD" "openssh-10.6p2" --sysconfdir=/etc/ssh --libexecdir=/usr/libexec/openssh \
    --with-privsep-path=/usr/share/empty.sshd --with-pid-dir=/run --with-default-path=/usr/bin \
    --without-pam --disable-security-key --disable-strip

finish

# It works, installed over what it was built with.
install_over
# ssh runs only as a user the system knows, and sshd checks for its
# own: the users, and the directory sshd needs, are made here.
grep -q "^root:" /etc/passwd 2>/dev/null || echo "root:x:0:0:root:/root:/usr/bin/bash" >>/etc/passwd
grep -q "^sshd:" /etc/passwd || echo "sshd:x:22:22:sshd:/usr/share/empty.sshd:/usr/bin/false" >>/etc/passwd
grep -q "^sshd:" /etc/group 2>/dev/null || echo "sshd:x:22:" >>/etc/group
mkdir -p /usr/share/empty.sshd /root
version=$(ssh -V 2>&1) || { echo "build.sh: ssh -V: $version" >&2; false; }
grep -q "^OpenSSH_10\.6p2, OpenSSL 3\.5\.9 " <<<"$version" || { echo "build.sh: ssh is $version" >&2; false; }
# A key, a signature made with it, and the signature checked.
ssh-keygen -q -t ed25519 -N "" -C made -f key
echo made >signed
ssh-keygen -q -Y sign -f key -n file signed
echo "made@example $(cat key.pub)" >allowed
ssh-keygen -q -Y verify -f allowed -I made@example -n file -s signed.sig <signed
# The server takes its configuration and a host key. It can't log
# anyone in here: its privilege separation drops to the sshd user, and
# the build is root only in a namespace that maps no other user and
# denies setgroups.
ssh-keygen -q -t ed25519 -N "" -f hostkey
/usr/sbin/sshd -t -f /etc/ssh/sshd_config -h "$PWD/hostkey"
