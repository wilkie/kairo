# Environment Contract: `linux-root` v1 (retired)

`environment:linux-root/<arch>` was the contract in which the build tree
was the guest's `/` (`specs/DECISIONS.md` §38 sub-decision 5). It is
retired: §41 folded it into `environment:linux/<arch>`
(`linux-v1.md`), where the guest is root, `/` is writable and holds its
dependencies at their install roots, the build tree is at `/kairo/src`,
and the artifact is what the build stages in `/kairo/package`. Nothing
built under `linux-root` was published, and no engine offers it.
