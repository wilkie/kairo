#!/usr/bin/env bash
# Archive the distfiles of live-bootstrap's python chain, guile, and
# autogen, and make the native object that runs them for x86, going on
# from pass 3's root (specs/DECISIONS.md §49).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. `kairo source fetch https://github.com/fosslinux/live-bootstrap`
#    archives live-bootstrap as a source object, as the earlier passes'
#    imports do; a node that has imported them holds it already.
# 2. Each distfile this pass's steps list is archived as a file source
#    with `kairo source fetch-file`, from upstream or else one of
#    live-bootstrap's mirrors: its maintainers' two, then archive.org.
#    The gnulib snapshots have no upstream URL; upstream's mirror.sh
#    makes them with `git archive`, and the mirrors serve those bytes.
#    ftp.unicode.org's files are fetched from www.unicode.org, which
#    serves them over https.
# 3. The native wrapper -- this directory's kairo.toml and
#    steps/manifest, and the start.sh and package.sh every later pass
#    shares (../live-bootstrap/pass/) and the musl and gcc splits
#    (../live-bootstrap/split-musl.sh, split-gcc.sh) -- is committed as
#    SHA-256 history and created as an object, or, with --wrapper,
#    committed on top of that existing wrapper object's history as its
#    next revision.
#
# Passes 1 to 3 (bootstrap/live-bootstrap/,
# bootstrap/live-bootstrap-perl/, and bootstrap/live-bootstrap-gcc-4.7.4/)
# must be imported: pass 3's artifact, data:live-bootstrap:x86:musl-1.2.5_2,
# is this build's input, and is built first, with the earlier passes', if
# it isn't held. Extra arguments go before the command in every `kairo`
# call, e.g. `--store <path>`. KAIRO names the kairo binary (default:
# kairo on PATH). Prints the wrapper's object id.
set -euo pipefail
shopt -s inherit_errexit

LIVE_BOOTSTRAP_URL=https://github.com/fosslinux/live-bootstrap

# sha256 name url... (from steps/<step>/sources at b1ceced7, upstream's
# URL first, then live-bootstrap's mirrors)
DISTFILES=(
    "9c772627c6ee77eea6a17b42927b8ee28ca05dc65d6a511062104baaf3d12294 CaseFolding-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/CaseFolding.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-12.1.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-12.1.0.txt"
    "abc8394c5bde62453118b00c1c5842160a04d7fffb2e829ee5426b846596d081 CompositionExclusions-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-12.1.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-12.1.0.txt"
    "a6eb7a8671fb532fbd88c37fd7b20b5b2e7dbfc8b121f74c14abe2947db0da68 DerivedCoreProperties-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-12.1.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-12.1.0.txt"
    "92dcdda84142194a1596f22180fcdf8c0e7f86897f09cc9203c7dc636c549f5f DerivedNormalizationProps-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/DerivedNormalizationProps.txt
     https://samuelt.me/pub/live-bootstrap/DerivedNormalizationProps-12.1.0.txt
     https://live-bootstrap.stikonas.eu/DerivedNormalizationProps-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedNormalizationProps-12.1.0.txt"
    "904500178b2e752635bef27aaed3a2a3718a100bce35ff96b3890be7a8315d8f EastAsianWidth-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-12.1.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-12.1.0.txt"
    "961f842fc70b5afd1d82c6645e68c10d1f701382aed38ae38cb2ff27f671903c LineBreak-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/LineBreak.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-12.1.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-12.1.0.txt"
    "ff61a0687d2f32c0dd1094254b8bde967883b43c2d4d50fd17531d498e41ab2c NameAliases-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/NameAliases.txt
     https://samuelt.me/pub/live-bootstrap/NameAliases-12.1.0.txt
     https://live-bootstrap.stikonas.eu/NameAliases-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/NameAliases-12.1.0.txt"
    "d3eb9a288ebeaf9de1237989f490705e287b6f610b59d2459fb1b7c2d8e39c39 NamedSequences-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/NamedSequences.txt
     https://samuelt.me/pub/live-bootstrap/NamedSequences-12.1.0.txt
     https://live-bootstrap.stikonas.eu/NamedSequences-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/NamedSequences-12.1.0.txt"
    "817ce2e9edca8e075a153f54b8f3b020345e37652cd2bda9b1495c366af17e7e SpecialCasing-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/SpecialCasing.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-12.1.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-12.1.0.txt"
    "93ab1acd8fd9d450463b50ae77eab151a7cda48f98b25b56baed8070f80fc936 UnicodeData-12.1.0.txt
     https://www.unicode.org/Public/12.1.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-12.1.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-12.1.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-12.1.0.txt"
    "6e4553f3b5fffe0d312df324d020ef1278d9595932ae03f4e8a2d427de83cdcd Unihan-12.1.0.zip
     https://www.unicode.org/Public/12.1.0/ucd/Unihan.zip
     https://samuelt.me/pub/live-bootstrap/Unihan-12.1.0.zip
     https://live-bootstrap.stikonas.eu/Unihan-12.1.0.zip
     https://archive.org/download/live-bootstrap-sources/Unihan-12.1.0.zip"
    "bdbffbbfc8ad4d3a6d01b5891510458f3d36f7170422af4ea2bed3211a73e8bb UnicodeData-13.0.0.txt
     https://www.unicode.org/Public/13.0.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-13.0.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-13.0.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-13.0.0.txt"
    "a566cd48687b2cd897e02501118b2413c14ae86d318f9abbbba97feb84189f0f CaseFolding-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/CaseFolding.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-14.0.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-14.0.0.txt"
    "3360762fc3295cea54ab251c31df621d05ba4b94d46c60eaac29aa16d70ad1e0 CompositionExclusions-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-14.0.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-14.0.0.txt"
    "e3eddd7d469cd1b0feed7528defad1a1cc7c6a9ceb0ae4446a6d10921ed2e7bc DerivedCoreProperties-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-14.0.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-14.0.0.txt"
    "b2c444c20730b097787fdf50bd7d6dd3fc5256ab8084f5b35b11c8776eca674c DerivedNormalizationProps-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/DerivedNormalizationProps.txt
     https://samuelt.me/pub/live-bootstrap/DerivedNormalizationProps-14.0.0.txt
     https://live-bootstrap.stikonas.eu/DerivedNormalizationProps-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedNormalizationProps-14.0.0.txt"
    "f901ac011aa32a09224d6555da71e2532c59c1d3381322829de0e3b880507250 EastAsianWidth-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-14.0.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-14.0.0.txt"
    "9e06e9f35c6959fb91dcc7993f90d58523c3079bc62c6b25f828b4cdebc5d70c LineBreak-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/LineBreak.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-14.0.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-14.0.0.txt"
    "14b3b677d33f95c51423dce6eef4a6a28b4b160451ecedee4b91edb6745cf4a3 NameAliases-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/NameAliases.txt
     https://samuelt.me/pub/live-bootstrap/NameAliases-14.0.0.txt
     https://live-bootstrap.stikonas.eu/NameAliases-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/NameAliases-14.0.0.txt"
    "db5745688affcdc0c3927a1ee0667018a96a7b24513f866d5235e98fef6c2436 NamedSequences-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/NamedSequences.txt
     https://samuelt.me/pub/live-bootstrap/NamedSequences-14.0.0.txt
     https://live-bootstrap.stikonas.eu/NamedSequences-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/NamedSequences-14.0.0.txt"
    "c667b45908fd269af25fd55d2fc5bbc157fb1b77675936e25c513ce32e080334 SpecialCasing-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/SpecialCasing.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-14.0.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-14.0.0.txt"
    "36018e68657fdcb3485f636630ffe8c8532e01c977703d2803f5b89d6c5feafb UnicodeData-14.0.0.txt
     https://www.unicode.org/Public/14.0.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-14.0.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-14.0.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-14.0.0.txt"
    "2ae4519b2b82cd4d15379c17e57bfb12c33c0f54da4977de03b2b04bcf11852d Unihan-14.0.0.zip
     https://www.unicode.org/Public/14.0.0/ucd/Unihan.zip
     https://samuelt.me/pub/live-bootstrap/Unihan-14.0.0.zip
     https://live-bootstrap.stikonas.eu/Unihan-14.0.0.zip
     https://archive.org/download/live-bootstrap-sources/Unihan-14.0.0.zip"
    "764f420cedfc8b43d9fec251c957a5d55fc45d40f6573f162990ed1dce7e36e0 ArabicShaping-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/ArabicShaping.txt
     https://samuelt.me/pub/live-bootstrap/ArabicShaping-16.0.0.txt
     https://live-bootstrap.stikonas.eu/ArabicShaping-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/ArabicShaping-16.0.0.txt"
    "d7afdadd1bbd66f5a663ac0e8f7958f18fd9491fc0bc59ec5877cb82db71db7d BidiMirroring-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/BidiMirroring.txt
     https://samuelt.me/pub/live-bootstrap/BidiMirroring-16.0.0.txt
     https://live-bootstrap.stikonas.eu/BidiMirroring-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/BidiMirroring-16.0.0.txt"
    "f3907b395d410f1b97342292ca6bc83dd12eb4b205f2a0c48efdef99e517d7b0 Blocks-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/Blocks.txt
     https://samuelt.me/pub/live-bootstrap/Blocks-16.0.0.txt
     https://live-bootstrap.stikonas.eu/Blocks-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/Blocks-16.0.0.txt"
    "6f1f9c588eb4a5c718d9e8f93b782685e5c7fec872cf05e8e6878053599e09bb CaseFolding-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/CaseFolding.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-16.0.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-16.0.0.txt"
    "89e83cf9cc8bef6c1f8bf77e42cf6f0341dfa42e66261f4dbe9b492e7a23c8ee CompositionExclusions-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-16.0.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-16.0.0.txt"
    "39d35161f2954497f69e08bdb9e701493f476a3d30222de20028feda36c1dabd DerivedCoreProperties-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-16.0.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-16.0.0.txt"
    "43adc76c0686a42cb370764eb8cfe2b2a45b10b855e5572a2db4a0eecce15d5b EastAsianWidth-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-16.0.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-16.0.0.txt"
    "e97e4259d0d20fab150b9c7b4b28abfae5cd78ca97e7f4ac6ed20d685d5f4a7c LineBreak-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/LineBreak.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-16.0.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-16.0.0.txt"
    "53d614508e2a0b2305a8aa21cd60d993de9326cdf65993660dfcce4503548583 PropList-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/PropList.txt
     https://samuelt.me/pub/live-bootstrap/PropList-16.0.0.txt
     https://live-bootstrap.stikonas.eu/PropList-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/PropList-16.0.0.txt"
    "9e88f0a677df47311106340be8ede2ecdacd9c1c931831218d2be6d5508e0039 Scripts-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/Scripts.txt
     https://samuelt.me/pub/live-bootstrap/Scripts-16.0.0.txt
     https://live-bootstrap.stikonas.eu/Scripts-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/Scripts-16.0.0.txt"
    "8d5de354eef79f2395a54c9c7dcebbaf3d30fc962d0f85611ea97aa973a0c451 SpecialCasing-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/SpecialCasing.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-16.0.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-16.0.0.txt"
    "ff58e5823bd095166564a006e47d111130813dcf8bf234ef79fa51a870edb48f UnicodeData-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-16.0.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-16.0.0.txt"
    "c29360bd6f7132811d701d29069541e827eb44bfc4c8fbde8c370d6982689dc1 GraphemeBreakProperty-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/auxiliary/GraphemeBreakProperty.txt
     https://samuelt.me/pub/live-bootstrap/GraphemeBreakProperty-16.0.0.txt
     https://live-bootstrap.stikonas.eu/GraphemeBreakProperty-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/GraphemeBreakProperty-16.0.0.txt"
    "476464e71a4b7b779b8ba7c5671f4338fea77da8e6b6b05fb82b3fdd14603779 WordBreakProperty-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/auxiliary/WordBreakProperty.txt
     https://samuelt.me/pub/live-bootstrap/WordBreakProperty-16.0.0.txt
     https://live-bootstrap.stikonas.eu/WordBreakProperty-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/WordBreakProperty-16.0.0.txt"
    "f1365a5173eee18e1f98b240cdc492e84a25f1ce7e0c9d1094eb29c41a22696a emoji-data-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/emoji/emoji-data.txt
     https://samuelt.me/pub/live-bootstrap/emoji-data-16.0.0.txt
     https://live-bootstrap.stikonas.eu/emoji-data-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/emoji-data-16.0.0.txt"
    "39afa01e680e27d0fd10b67a9b27be13fbaa3d0efecfb5be45991de9a0d267d0 ArabicShaping-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/ArabicShaping.txt
     https://samuelt.me/pub/live-bootstrap/ArabicShaping-17.0.0.txt
     https://live-bootstrap.stikonas.eu/ArabicShaping-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/ArabicShaping-17.0.0.txt"
    "a2f16fb873ab4fcdf3221cb1a8a85a134ddd6ed03603181823ff5206af3741ce BidiMirroring-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/BidiMirroring.txt
     https://samuelt.me/pub/live-bootstrap/BidiMirroring-17.0.0.txt
     https://live-bootstrap.stikonas.eu/BidiMirroring-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/BidiMirroring-17.0.0.txt"
    "c0edefaf1a19771e830a82735472716af6bf3c3975f6c2a23ffbe2580fbbcb15 Blocks-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/Blocks.txt
     https://samuelt.me/pub/live-bootstrap/Blocks-17.0.0.txt
     https://live-bootstrap.stikonas.eu/Blocks-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/Blocks-17.0.0.txt"
    "ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183 CaseFolding-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-17.0.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-17.0.0.txt"
    "2f239196ef3b5b61db5cc476e9bd80f534d15aa1b74e1be1dea5d042a344c85f CompositionExclusions-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-17.0.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-17.0.0.txt"
    "24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08 DerivedCoreProperties-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-17.0.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-17.0.0.txt"
    "ea7ce50f3444a050333448dffef1cadd9325af55cbb764b4a2280faf52170a33 EastAsianWidth-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-17.0.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-17.0.0.txt"
    "e6a18fa91f8f6a6f8e534b1d3f128c21ada45bfe152eb6b1bcc5e15fd8ac92e6 LineBreak-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/LineBreak.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-17.0.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-17.0.0.txt"
    "793f6f1e4d15fd90f05ae66460191dc4d75d1fea90136a25f30dd6a4cb950eac NameAliases-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/NameAliases.txt
     https://samuelt.me/pub/live-bootstrap/NameAliases-17.0.0.txt
     https://live-bootstrap.stikonas.eu/NameAliases-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/NameAliases-17.0.0.txt"
    "130dcddcaadaf071008bdfce1e7743e04fdfbc910886f017d9f9ac931d8c64dd PropList-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/PropList.txt
     https://samuelt.me/pub/live-bootstrap/PropList-17.0.0.txt
     https://live-bootstrap.stikonas.eu/PropList-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/PropList-17.0.0.txt"
    "9f5e50d3abaee7d6ce09480f325c706f485ae3240912527e651954d2d6b035bf Scripts-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/Scripts.txt
     https://samuelt.me/pub/live-bootstrap/Scripts-17.0.0.txt
     https://live-bootstrap.stikonas.eu/Scripts-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/Scripts-17.0.0.txt"
    "efc25faf19de21b92c1194c111c932e03d2a5eaf18194e33f1156e96de4c9588 SpecialCasing-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/SpecialCasing.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-17.0.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-17.0.0.txt"
    "2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c UnicodeData-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-17.0.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-17.0.0.txt"
    "d6b51d1d2ae5c33b451b7ed994b48f1f4dc62b2272a5831e7fd418514a6bae89 GraphemeBreakProperty-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakProperty.txt
     https://samuelt.me/pub/live-bootstrap/GraphemeBreakProperty-17.0.0.txt
     https://live-bootstrap.stikonas.eu/GraphemeBreakProperty-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/GraphemeBreakProperty-17.0.0.txt"
    "72274cac1e6b919507db35655c3e175aa27274668a1ece95c28d2069f2ad9852 WordBreakProperty-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/auxiliary/WordBreakProperty.txt
     https://samuelt.me/pub/live-bootstrap/WordBreakProperty-17.0.0.txt
     https://live-bootstrap.stikonas.eu/WordBreakProperty-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/WordBreakProperty-17.0.0.txt"
    "2cb2bb9455cda83e8481541ecf5b6dfda66a3bb89efa3fa7c5297eccf607b72b emoji-data-17.0.0.txt
     https://www.unicode.org/Public/17.0.0/ucd/emoji/emoji-data.txt
     https://samuelt.me/pub/live-bootstrap/emoji-data-17.0.0.txt
     https://live-bootstrap.stikonas.eu/emoji-data-17.0.0.txt
     https://archive.org/download/live-bootstrap-sources/emoji-data-17.0.0.txt"
    "f41d967bc458ee106f0c3948bfad71cd0860d96c49304e3fd02eaf2bbae4b6d9 UnicodeData-3.0.0.txt
     https://www.unicode.org/Public/3.0-Update/UnicodeData-3.0.0.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-3.0.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-3.0.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-3.0.0.txt"
    "909eef4adbeddbdddcd9487c856fe8cdbb8912aa8eb315ed7885b6ef65f4dc4c PropList-3.0.1.txt
     https://www.unicode.org/Public/3.0-Update1/PropList-3.0.1.txt
     https://samuelt.me/pub/live-bootstrap/PropList-3.0.1.txt
     https://live-bootstrap.stikonas.eu/PropList-3.0.1.txt
     https://archive.org/download/live-bootstrap-sources/PropList-3.0.1.txt"
    "370f3d1e79a52791c42065946711f4eddb6d9820726afd0e436a3c50360475a9 CaseFolding-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/CaseFolding-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-3.2.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-3.2.0.txt"
    "1d3a450d0f39902710df4972ac4a60ec31fbcb54ffd4d53cd812fc1200c732cb CompositionExclusions-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/CompositionExclusions-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-3.2.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-3.2.0.txt"
    "787419dde91701018d7ad4f47432eaa55af14e3fe3fe140a11e4bbf3db18bb4c DerivedCoreProperties-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/DerivedCoreProperties-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-3.2.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-3.2.0.txt"
    "bab49295e5f9064213762447224ccd83cea0cced0db5dcfc96f9c8a935ef67ee DerivedNormalizationProps-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/DerivedNormalizationProps-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/DerivedNormalizationProps-3.2.0.txt
     https://live-bootstrap.stikonas.eu/DerivedNormalizationProps-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedNormalizationProps-3.2.0.txt"
    "ce19f35ffca911bf492aab6c0d3f6af3d1932f35d2064cf2fe14e10be29534cb EastAsianWidth-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/EastAsianWidth-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-3.2.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-3.2.0.txt"
    "d693ef2a603d07e20b769ef8ba29afca39765588a03e3196294e5be8638ca735 LineBreak-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/LineBreak-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-3.2.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-3.2.0.txt"
    "1f7913b74dddff55ee566f6220aa9e465bae6f27709fc21d353b04adb8572b37 SpecialCasing-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/SpecialCasing-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-3.2.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-3.2.0.txt"
    "5e444028b6e76d96f9dc509609c5e3222bf609056f35e5fcde7e6fb8a58cd446 UnicodeData-3.2.0.txt
     https://www.unicode.org/Public/3.2-Update/UnicodeData-3.2.0.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-3.2.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-3.2.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-3.2.0.txt"
    "0582b888c4ebab6e3ce8d340c74788f1a68ca662713a1065b9a007f24bb4fe46 Unihan-3.2.0.zip
     https://www.unicode.org/Public/3.2-Update/Unihan-3.2.0.zip
     https://samuelt.me/pub/live-bootstrap/Unihan-3.2.0.zip
     https://live-bootstrap.stikonas.eu/Unihan-3.2.0.zip
     https://archive.org/download/live-bootstrap-sources/Unihan-3.2.0.zip"
    "c9dba71655b8787c381b3421ba2bf9a3df9e6168b5835f93fd3931c1eeff90cf Blocks-4.0.1.txt
     https://www.unicode.org/Public/4.0-Update1/Blocks-4.0.1.txt
     https://samuelt.me/pub/live-bootstrap/Blocks-4.0.1.txt
     https://live-bootstrap.stikonas.eu/Blocks-4.0.1.txt
     https://archive.org/download/live-bootstrap-sources/Blocks-4.0.1.txt"
    "de54d68f2df927a20ea4a83dfd74a543ca7f3e3df2441576e7f6a185ec6fa217 UnicodeData-4.0.1.txt
     https://www.unicode.org/Public/4.0-Update1/UnicodeData-4.0.1.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-4.0.1.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-4.0.1.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-4.0.1.txt"
    "1003a6896078e77532a017b135762501ff0a540ba33694e32b6177f093ebe6b2 CompositionExclusions-4.1.0.txt
     https://www.unicode.org/Public/4.1.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-4.1.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-4.1.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-4.1.0.txt"
    "089ed5b2becd3196e61124d36e968474d3b7152cb5a3fb56594c34ab1e698e92 EastAsianWidth-4.1.0.txt
     https://www.unicode.org/Public/4.1.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-4.1.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-4.1.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-4.1.0.txt"
    "a9f03f6a061ee210c53e33782288a208bed48c65c70d307b2b214989cedfdab0 UnicodeData-4.1.0.txt
     https://www.unicode.org/Public/4.1.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-4.1.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-4.1.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-4.1.0.txt"
    "683b094f2bdd0ab132c0bac293a5404626dd858a53b5364b3b6b525323c5a5e4 CompositionExclusions-5.1.0.txt
     https://www.unicode.org/Public/5.1.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-5.1.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-5.1.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-5.1.0.txt"
    "8f54c77587fee99facc2f28b94e748dfdda5da44f42adab31a65f88b63587ae0 DerivedCoreProperties-5.1.0.txt
     https://www.unicode.org/Public/5.1.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-5.1.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-5.1.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-5.1.0.txt"
    "4fc8cbfa1eed578cdda0768fb4a4ace5443f807c1f652e36a6bd768e81c2c2a3 DerivedNormalizationProps-5.1.0.txt
     https://www.unicode.org/Public/5.1.0/ucd/DerivedNormalizationProps.txt
     https://samuelt.me/pub/live-bootstrap/DerivedNormalizationProps-5.1.0.txt
     https://live-bootstrap.stikonas.eu/DerivedNormalizationProps-5.1.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedNormalizationProps-5.1.0.txt"
    "a0d8abf08d08f3e61875aed6011cb70c61dd8ea61089e6ad9b6cf524d8fba0f2 EastAsianWidth-5.1.0.txt
     https://www.unicode.org/Public/5.1.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-5.1.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-5.1.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-5.1.0.txt"
    "8bd83e9c4e339728ecd532c5b174de5beb9cb4bab5db14e44fcd03ccb2e2c1b5 UnicodeData-5.1.0.txt
     https://www.unicode.org/Public/5.1.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-5.1.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-5.1.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-5.1.0.txt"
    "4c0bece13821a24f469bb8d16ea33fc7da6436b7ebe64c78635673dbfaa88edc CaseFolding-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/CaseFolding.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-6.1.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-6.1.0.txt"
    "21124f9d38372d68e09c67bcb64694fd4bca0c9cb39c576b1f095554c4ea9693 CompositionExclusions-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-6.1.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-6.1.0.txt"
    "a03e62ba5fa9c6f327b6e6cfc5d014f59af9b262b768dd9a6aaa39d205dd8b7a DerivedCoreProperties-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-6.1.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-6.1.0.txt"
    "d028f7eccab4998f8d7a6b15703b088e26ff6ee1f2dbc0939ae872c213de8620 DerivedNormalizationProps-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/DerivedNormalizationProps.txt
     https://samuelt.me/pub/live-bootstrap/DerivedNormalizationProps-6.1.0.txt
     https://live-bootstrap.stikonas.eu/DerivedNormalizationProps-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedNormalizationProps-6.1.0.txt"
    "d591c24b702c1b025b58ca6168746f713b657c6e252c268f52cb07758f428067 EastAsianWidth-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-6.1.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-6.1.0.txt"
    "7b7e2cf582ef7f24fd2747a4ef1a50934c15a0fc0ab10ce737d5e3e47bebde0d LineBreak-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/LineBreak.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-6.1.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-6.1.0.txt"
    "7253bd84e20d34491b2b124a85ca84bd2cd5d113e4957aebae92f0e3c21f0a45 NameAliases-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/NameAliases.txt
     https://samuelt.me/pub/live-bootstrap/NameAliases-6.1.0.txt
     https://live-bootstrap.stikonas.eu/NameAliases-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/NameAliases-6.1.0.txt"
    "60c88b6e3ceec871cc6b7e2d552453f88eef0f40ff2188d9cec7021c2debd36a NamedSequences-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/NamedSequences.txt
     https://samuelt.me/pub/live-bootstrap/NamedSequences-6.1.0.txt
     https://live-bootstrap.stikonas.eu/NamedSequences-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/NamedSequences-6.1.0.txt"
    "7d047fe1aa8a68cc12101427cf03bfbce83201ee277e907822901735f0bfee3c SpecialCasing-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/SpecialCasing.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-6.1.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-6.1.0.txt"
    "3066262585a3c4f407b16db787e6d3a6e033b90f27405b6c76d1babefffca6ad UnicodeData-6.1.0.txt
     https://www.unicode.org/Public/6.1.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-6.1.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-6.1.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-6.1.0.txt"
    "8ca508ef1bc7eba8c102710016d8510f871f69bdcc74ff877c33d01bb799a38f Unihan-6.1.0.zip
     https://www.unicode.org/Public/6.1.0/ucd/Unihan.zip
     https://samuelt.me/pub/live-bootstrap/Unihan-6.1.0.zip
     https://live-bootstrap.stikonas.eu/Unihan-6.1.0.zip
     https://archive.org/download/live-bootstrap-sources/Unihan-6.1.0.zip"
    "21323e682a2b34400c6af4ab57b9775b7e716150428f092bac5b005a88ab8f42 CaseFolding-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/CaseFolding.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-6.3.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-6.3.0.txt"
    "4ba8ea079ffbffc0025fc31009e95726864feda90d2845c9363c0c40ded8511c CompositionExclusions-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-6.3.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-6.3.0.txt"
    "790826f4cfa82c5845ab4040b5e811f1e67bf1ec4c88cdbf722795c3292b0102 DerivedCoreProperties-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-6.3.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-6.3.0.txt"
    "c5e867ae043fe5d1cf713150d859356bfdcdba291c39f584af0bfb943f1a9743 DerivedNormalizationProps-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/DerivedNormalizationProps.txt
     https://samuelt.me/pub/live-bootstrap/DerivedNormalizationProps-6.3.0.txt
     https://live-bootstrap.stikonas.eu/DerivedNormalizationProps-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedNormalizationProps-6.3.0.txt"
    "bbdf9281767ca4601af3623b62c26ecb834a9f4c46eec629d82339b006da00d8 EastAsianWidth-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-6.3.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-6.3.0.txt"
    "6a38069025127a60f4a809e788fbbd1bb6b95ac8d1bd62e6a78d7870357f3486 LineBreak-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/LineBreak.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-6.3.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-6.3.0.txt"
    "a11bed87ec6f264edcf84d581dd2d7ac8ed7ac1c3b2ccb54a83077fdbd34133e NameAliases-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/NameAliases.txt
     https://samuelt.me/pub/live-bootstrap/NameAliases-6.3.0.txt
     https://live-bootstrap.stikonas.eu/NameAliases-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/NameAliases-6.3.0.txt"
    "91fc69ff68b1a89e5f7270545547c747624bc96b0e6c23a791d4265d2fa1f988 NamedSequences-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/NamedSequences.txt
     https://samuelt.me/pub/live-bootstrap/NamedSequences-6.3.0.txt
     https://live-bootstrap.stikonas.eu/NamedSequences-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/NamedSequences-6.3.0.txt"
    "9edafba261e23e72f6e21e3d85d7f15dd4866f38004ab3bfdc6f7057c589d034 SpecialCasing-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/SpecialCasing.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-6.3.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-6.3.0.txt"
    "3f76924f0410ca8ae0e9b5c59bd1ba03196293c32616204b393300f091f52013 UnicodeData-6.3.0.txt
     https://www.unicode.org/Public/6.3.0/ucd/UnicodeData.txt
     https://samuelt.me/pub/live-bootstrap/UnicodeData-6.3.0.txt
     https://live-bootstrap.stikonas.eu/UnicodeData-6.3.0.txt
     https://archive.org/download/live-bootstrap-sources/UnicodeData-6.3.0.txt"
    "9e408d71e3aba4ff68f5085569bc1c31c9751f9779f55cf877c222467732991f Unihan-6.3.0.zip
     https://www.unicode.org/Public/6.3.0/ucd/Unihan.zip
     https://samuelt.me/pub/live-bootstrap/Unihan-6.3.0.zip
     https://live-bootstrap.stikonas.eu/Unihan-6.3.0.zip
     https://archive.org/download/live-bootstrap-sources/Unihan-6.3.0.zip"
    "6bad4dabcdf5940227c7d81fab130dcb18a77850b5d79de28b5dc4e047b0aaac CP437.TXT
     https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT
     https://samuelt.me/pub/live-bootstrap/CP437.TXT
     https://live-bootstrap.stikonas.eu/CP437.TXT
     https://archive.org/download/live-bootstrap-sources/CP437.TXT"
    "dfcaec925a788b0ba41e51bc6d16e21b0e98b4c7af9b79395090add75f5e506f Text-CSV-2.06.tar.gz
     https://cpan.metacpan.org/authors/id/I/IS/ISHIGAKI/Text-CSV-2.06.tar.gz
     https://samuelt.me/pub/live-bootstrap/Text-CSV-2.06.tar.gz
     https://live-bootstrap.stikonas.eu/Text-CSV-2.06.tar.gz
     https://archive.org/download/live-bootstrap-sources/Text-CSV-2.06.tar.gz"
    "42e5194ad06ab6ffb806c83eb99c03462b495d979cda782f3c72c08af833cd4e gc-8.2.12.tar.gz
     https://github.com/bdwgc/bdwgc/releases/download/v8.2.12/gc-8.2.12.tar.gz
     https://samuelt.me/pub/live-bootstrap/gc-8.2.12.tar.gz
     https://live-bootstrap.stikonas.eu/gc-8.2.12.tar.gz
     https://archive.org/download/live-bootstrap-sources/gc-8.2.12.tar.gz"
    "f3a3082a23b37c293a4fcd1053147b371f2ff91fa7ea1b2a52e335676bac82dc libffi-3.5.2.tar.gz
     https://github.com/libffi/libffi/releases/download/v3.5.2/libffi-3.5.2.tar.gz
     https://samuelt.me/pub/live-bootstrap/libffi-3.5.2.tar.gz
     https://live-bootstrap.stikonas.eu/libffi-3.5.2.tar.gz
     https://archive.org/download/live-bootstrap-sources/libffi-3.5.2.tar.gz"
    "b6a5f44b7eb69e3fa35dbf15524405b44837a481d43d81daddde3ff21fcbb8e9 openssl-3.6.0.tar.gz
     https://github.com/openssl/openssl/releases/download/openssl-3.6.0/openssl-3.6.0.tar.gz
     https://samuelt.me/pub/live-bootstrap/openssl-3.6.0.tar.gz
     https://live-bootstrap.stikonas.eu/openssl-3.6.0.tar.gz
     https://archive.org/download/live-bootstrap-sources/openssl-3.6.0.tar.gz"
    "f8a13466b48faa3ba99fe17a069e71c9ab006d9b1cfabe699f8c60a47d5bb49a autogen-5.18.16.tar.xz
     https://mirrors.kernel.org/gnu/autogen/rel5.18.16/autogen-5.18.16.tar.xz
     https://samuelt.me/pub/live-bootstrap/autogen-5.18.16.tar.xz
     https://live-bootstrap.stikonas.eu/autogen-5.18.16.tar.xz
     https://archive.org/download/live-bootstrap-sources/autogen-5.18.16.tar.xz"
    "d1fb86e260cfe7da6031f94d2e44c0da55903dbae0a2fa0fae78c91ae1b56f00 gettext-0.26.tar.xz
     https://mirrors.kernel.org/gnu/gettext/gettext-0.26.tar.xz
     https://samuelt.me/pub/live-bootstrap/gettext-0.26.tar.xz
     https://live-bootstrap.stikonas.eu/gettext-0.26.tar.xz
     https://archive.org/download/live-bootstrap-sources/gettext-0.26.tar.xz"
    "fd87e0aba7e43ae054837afd6cd4db03a3f2693deb3619085e6ed9d8d9604ad8 gperf-3.3.tar.gz
     https://mirrors.kernel.org/gnu/gperf/gperf-3.3.tar.gz
     https://samuelt.me/pub/live-bootstrap/gperf-3.3.tar.gz
     https://live-bootstrap.stikonas.eu/gperf-3.3.tar.gz
     https://archive.org/download/live-bootstrap-sources/gperf-3.3.tar.gz"
    "2649b27c0e90e632eadcd757be06c6e9a4f48d941de51e7c0f83ff76408a07b9 grep-3.12.tar.xz
     https://mirrors.kernel.org/gnu/grep/grep-3.12.tar.xz
     https://samuelt.me/pub/live-bootstrap/grep-3.12.tar.xz
     https://live-bootstrap.stikonas.eu/grep-3.12.tar.xz
     https://archive.org/download/live-bootstrap-sources/grep-3.12.tar.xz"
    "818c79d236657a7fa96fb364137cc7b41b3bdee0d65c6174ca03769559579460 guile-3.0.11.tar.xz
     https://mirrors.kernel.org/gnu/guile/guile-3.0.11.tar.xz
     https://samuelt.me/pub/live-bootstrap/guile-3.0.11.tar.xz
     https://live-bootstrap.stikonas.eu/guile-3.0.11.tar.xz
     https://archive.org/download/live-bootstrap-sources/guile-3.0.11.tar.xz"
    "f57d86c70620271bfceb7a9be0c81744a033f08adc7ceba832c9917ab3e691b7 guile-3.0.7.tar.xz
     https://mirrors.kernel.org/gnu/guile/guile-3.0.7.tar.xz
     https://samuelt.me/pub/live-bootstrap/guile-3.0.7.tar.xz
     https://live-bootstrap.stikonas.eu/guile-3.0.7.tar.xz
     https://archive.org/download/live-bootstrap-sources/guile-3.0.7.tar.xz"
    "67d88430892527861903788868c77802a217b0959990f7449f2976126a307763 libunistring-1.4.1.tar.xz
     https://mirrors.kernel.org/gnu/libunistring/libunistring-1.4.1.tar.xz
     https://samuelt.me/pub/live-bootstrap/libunistring-1.4.1.tar.xz
     https://live-bootstrap.stikonas.eu/libunistring-1.4.1.tar.xz
     https://archive.org/download/live-bootstrap-sources/libunistring-1.4.1.tar.xz"
    "f79b0cfea71b37a8eeec8490db6c5f7ae7719c35587f21edb0617f370eeff633 sed-4.8.tar.xz
     https://mirrors.kernel.org/gnu/sed/sed-4.8.tar.xz
     https://samuelt.me/pub/live-bootstrap/sed-4.8.tar.xz
     https://live-bootstrap.stikonas.eu/sed-4.8.tar.xz
     https://archive.org/download/live-bootstrap-sources/sed-4.8.tar.xz"
    "0329d7788fbef113fa82cb80889ca197a344ce0df7646fe000974c5d714363a6 texinfo-7.2.tar.xz
     https://mirrors.kernel.org/gnu/texinfo/texinfo-7.2.tar.xz
     https://samuelt.me/pub/live-bootstrap/texinfo-7.2.tar.xz
     https://live-bootstrap.stikonas.eu/texinfo-7.2.tar.xz
     https://archive.org/download/live-bootstrap-sources/texinfo-7.2.tar.xz"
    "bc34b498d87dfaadc10a2fa52e38dacc72a16fbc9ae477a294450667fa8abf49 country-codes.csv
     https://raw.githubusercontent.com/datasets/country-codes/1991017dfce2e8cdea0c1d6f20341bd0f9baa225/data/country-codes.csv
     https://samuelt.me/pub/live-bootstrap/country-codes.csv
     https://live-bootstrap.stikonas.eu/country-codes.csv
     https://archive.org/download/live-bootstrap-sources/country-codes.csv"
    "eb722fa698fb7e8823b835d9fd263e4cdb8f1c7b0d234edf7f0e3bd2ccbb2c79 rfc3454.txt
     https://www.ietf.org/rfc/rfc3454.txt
     https://samuelt.me/pub/live-bootstrap/rfc3454.txt
     https://live-bootstrap.stikonas.eu/rfc3454.txt
     https://archive.org/download/live-bootstrap-sources/rfc3454.txt"
    "42b71885e4dc885559fda5ad059fd81838cf4782cedb5fd08cc3431f7d067371 ISO-639-2_utf-8.txt
     https://www.loc.gov/standards/iso639-2/ISO-639-2_utf-8.txt
     https://samuelt.me/pub/live-bootstrap/ISO-639-2_utf-8.txt
     https://live-bootstrap.stikonas.eu/ISO-639-2_utf-8.txt
     https://archive.org/download/live-bootstrap-sources/ISO-639-2_utf-8.txt"
    "98557b819a42d2093b41d8637302d1311b81f627af9ad20036357d7eb2813872 Python-2.0.1.tgz
     https://www.python.org/ftp/python/2.0.1/Python-2.0.1.tgz
     https://samuelt.me/pub/live-bootstrap/Python-2.0.1.tgz
     https://live-bootstrap.stikonas.eu/Python-2.0.1.tgz
     https://archive.org/download/live-bootstrap-sources/Python-2.0.1.tgz"
    "969a9891dce9f50b13e54f9890acaf2be66715a5895bf9b11111f320c205b90e Python-2.3.7.tgz
     https://www.python.org/ftp/python/2.3.7/Python-2.3.7.tgz
     https://samuelt.me/pub/live-bootstrap/Python-2.3.7.tgz
     https://live-bootstrap.stikonas.eu/Python-2.3.7.tgz
     https://archive.org/download/live-bootstrap-sources/Python-2.3.7.tgz"
    "57e04484de051decd4741fb4a4a3f543becc9a219af8b8063b5541e270f26dcc Python-2.5.6.tar.bz2
     https://www.python.org/ftp/python/2.5.6/Python-2.5.6.tar.bz2
     https://samuelt.me/pub/live-bootstrap/Python-2.5.6.tar.bz2
     https://live-bootstrap.stikonas.eu/Python-2.5.6.tar.bz2
     https://archive.org/download/live-bootstrap-sources/Python-2.5.6.tar.bz2"
    "3a72a21528f0751e89151744350dd12004131d312d47b935ce8041b070c90361 Python-3.1.5.tar.bz2
     https://www.python.org/ftp/python/3.1.5/Python-3.1.5.tar.bz2
     https://samuelt.me/pub/live-bootstrap/Python-3.1.5.tar.bz2
     https://live-bootstrap.stikonas.eu/Python-3.1.5.tar.bz2
     https://archive.org/download/live-bootstrap-sources/Python-3.1.5.tar.bz2"
    "85879192f2cffd56cb16c092905949ebf3e5e394b7f764723529637901dfb58f Python-3.11.1.tar.xz
     https://www.python.org/ftp/python/3.11.1/Python-3.11.1.tar.xz
     https://samuelt.me/pub/live-bootstrap/Python-3.11.1.tar.xz
     https://live-bootstrap.stikonas.eu/Python-3.11.1.tar.xz
     https://archive.org/download/live-bootstrap-sources/Python-3.11.1.tar.xz"
    "85f60c327501c36bc18c33370c14d472801e6af2f901dafbba056f61685429fe Python-3.3.7.tar.xz
     https://www.python.org/ftp/python/3.3.7/Python-3.3.7.tar.xz
     https://samuelt.me/pub/live-bootstrap/Python-3.3.7.tar.xz
     https://live-bootstrap.stikonas.eu/Python-3.3.7.tar.xz
     https://archive.org/download/live-bootstrap-sources/Python-3.3.7.tar.xz"
    "d46a8f6fe91679e199c671b1b0a30aaf172d2acb5bcab25beb35f16c3d195b4e Python-3.4.10.tar.xz
     https://www.python.org/ftp/python/3.4.10/Python-3.4.10.tar.xz
     https://samuelt.me/pub/live-bootstrap/Python-3.4.10.tar.xz
     https://live-bootstrap.stikonas.eu/Python-3.4.10.tar.xz
     https://archive.org/download/live-bootstrap-sources/Python-3.4.10.tar.xz"
    "d85dbb3774132473d8081dcb158f34a10ccad7a90b96c7e50ea4bb61f5ce4562 Python-3.8.16.tar.xz
     https://www.python.org/ftp/python/3.8.16/Python-3.8.16.tar.xz
     https://samuelt.me/pub/live-bootstrap/Python-3.8.16.tar.xz
     https://live-bootstrap.stikonas.eu/Python-3.8.16.tar.xz
     https://archive.org/download/live-bootstrap-sources/Python-3.8.16.tar.xz"
    "aeffc74d35b7bc3c0ed636a784fee4daa05764c3c2a4f165841ece5c9095f9e6 cwm4-3c2d71a3fa598cf336c2fa2d8e64332b62c1ffba.tar.gz
     https://github.com/CarloWood/cwm4/archive/3c2d71a3fa598cf336c2fa2d8e64332b62c1ffba.tar.gz
     https://samuelt.me/pub/live-bootstrap/cwm4-3c2d71a3fa598cf336c2fa2d8e64332b62c1ffba.tar.gz
     https://live-bootstrap.stikonas.eu/cwm4-3c2d71a3fa598cf336c2fa2d8e64332b62c1ffba.tar.gz
     https://archive.org/download/live-bootstrap-sources/cwm4-3c2d71a3fa598cf336c2fa2d8e64332b62c1ffba.tar.gz"
    "953ba180b18acff188a0a8700770c7cf2fc97e1683c7b9699a5a748b542ccdd5 autogen-5.18.16-v1.0.1.tar.gz
     https://github.com/schierlm/gnu-autogen-bootstrapping/archive/refs/tags/autogen-5.18.16-v1.0.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/autogen-5.18.16-v1.0.1.tar.gz
     https://live-bootstrap.stikonas.eu/autogen-5.18.16-v1.0.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/autogen-5.18.16-v1.0.1.tar.gz"
    "14cda9c416506dfadf60c14fc623ff01ef99b87564a78d0a29c5d17143c97609 guile-3.0.7.tar.gz
     https://github.com/schierlm/guile-psyntax-bootstrapping/archive/refs/tags/guile-3.0.7.tar.gz
     https://samuelt.me/pub/live-bootstrap/guile-3.0.7.tar.gz
     https://live-bootstrap.stikonas.eu/guile-3.0.7.tar.gz
     https://archive.org/download/live-bootstrap-sources/guile-3.0.7.tar.gz"
    "d490df9133f8871d4678eb9e899313c3b238e455d45aaf64c9b10b76495b19e7 libxml2-v2.9.9.tar.gz
     https://gitlab.gnome.org/GNOME/libxml2/-/archive/v2.9.9/libxml2-v2.9.9.tar.gz
     https://samuelt.me/pub/live-bootstrap/libxml2-v2.9.9.tar.gz
     https://live-bootstrap.stikonas.eu/libxml2-v2.9.9.tar.gz
     https://archive.org/download/live-bootstrap-sources/libxml2-v2.9.9.tar.gz"
    "0c04ab2f7ce13c4a1c06c4abc7dfe75312aad89b8b0a1068e5e563787eb56632 autogen-5.18.16.tar.gz
     https://git.savannah.gnu.org/cgit/autogen.git/snapshot/autogen-5.18.16.tar.gz
     https://samuelt.me/pub/live-bootstrap/autogen-5.18.16.tar.gz
     https://live-bootstrap.stikonas.eu/autogen-5.18.16.tar.gz
     https://archive.org/download/live-bootstrap-sources/autogen-5.18.16.tar.gz"
    "fc9973f1a9243fdc4b98d33d7704f3c71bfdc4c2ef96899b8f28cade7290a714 gnulib-356a414e.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-356a414e.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-356a414e.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-356a414e.tar.gz"
    "edc3d33a24008a96e7d745812659c4b89f2c89e8ea74688f1098eac5b40f7890 gnulib-3773db6.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-3773db6.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-3773db6.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-3773db6.tar.gz"
    "04541986073955243d0ce7aa0e5f4017298ae17892a0064ae0b5513808cdad1b gnulib-6d64a31.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-6d64a31.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-6d64a31.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-6d64a31.tar.gz"
    "e207c0bb72093c3a72dde302fcfaa1dbda12a62172d47b73565883a92209ebab gnulib-8f4538a5.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-8f4538a5.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-8f4538a5.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-8f4538a5.tar.gz"
    "f9aad85de1f41d57c9368d304020ffbf354a5e56db1297f022c3d12181134e56 gnulib-901694b9.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-901694b9.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-901694b9.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-901694b9.tar.gz"
    "b2f5ee417643f729b6c679fbc0c3cd5e448fa17d2b0a9fd612a8e203573a8757 gnulib-b08ee1d.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-b08ee1d.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-b08ee1d.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-b08ee1d.tar.gz"
    "02fa193c8b8242dcae324c9bb2d694633d891d32d50e5a8cf79f4053323aec70 gnulib-b5eb878.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-b5eb878.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-b5eb878.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-b5eb878.tar.gz"
    "12cfa21abf618a274017d6b18e95fc6582519d7c08e2403e5c5772ccdd5b85f4 gnulib-d279bc.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-d279bc.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-d279bc.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-d279bc.tar.gz"
    "6c6e35a6028317dcea2ddacc9551e2d03308fb9c85c5b7f636a866689d304629 gnulib-d82702e.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-d82702e.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-d82702e.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-d82702e.tar.gz"
    "4442a6e3d16ce13ea6a2920115321ef38050f2b1bfa0b6ad5723375fefc7b684 which-2.23.tar.gz
     https://samuelt.me/pub/live-bootstrap/which-2.23.tar.gz
     https://live-bootstrap.stikonas.eu/which-2.23.tar.gz
     https://archive.org/download/live-bootstrap-sources/which-2.23.tar.gz"
)

KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)

kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}
# shellcheck source=../wrapper.sh
. "$HERE/../wrapper.sh"
parse_import_args "$@"

# 1. live-bootstrap, and the submodules it pins.
kairo source fetch "$LIVE_BOOTSTRAP_URL" --actor "$ACTOR" --require-complete >&2

# 2. The distfiles, each from the first mirror that serves its bytes.
fetch_distfiles

# 3. The native wrapper (../wrapper.sh).
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir "$WORK/steps"
cp "$HERE/kairo.toml" "$HERE/../live-bootstrap/pass/start.sh" "$HERE/../live-bootstrap/pass/package.sh" \
    "$HERE/../live-bootstrap/split-musl.sh" "$HERE/../live-bootstrap/split-gcc.sh" "$WORK/"
cp "$HERE/steps/manifest" "$WORK/steps/"
commit_wrapper "live-bootstrap b1ceced7 x86 pass 4: python, guile, and autogen" kairo.toml start.sh package.sh split-musl.sh split-gcc.sh steps/manifest

cat <<OUT
wrapper = $WRAPPER

# With passes 1 to 3 imported, and the same config:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
OUT
