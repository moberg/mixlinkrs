#!/bin/bash
#
# Builds the VST3 SDK hosting sources into a single static library that the
# MixLink target links against.
#
# The SDK ships a CMake build, but MixLink keeps a hand-maintained .pbxproj and
# has no CMake dependency, so this compiles the hosting subset directly with
# clang++. Everything not needed to load and run a plugin (VSTGUI, the plugin
# wrappers, the test suite) is left out.
#
# Usage: Scripts/build-vst3-sdk.sh
# Output: build/vst3sdk/libvst3sdk.a
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SDK="$ROOT/Vendor/vst3sdk"
OUT="${MIXLINK_VST3_BUILD_DIR:-$ROOT/build/vst3sdk}"
LIB="$OUT/libvst3sdk.a"
ARCHS="${MIXLINK_VST3_ARCHS:-arm64 x86_64}"
DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"

# The SDK lives in Vendor/vst3sdk, untracked, and is fetched on demand. Only the
# three repos below are needed; the vst3sdk umbrella repo also pulls in VSTGUI,
# which MixLink does not use.
VST3_SDK_TAG="${MIXLINK_VST3_TAG:-v3.8.1_build_84}"
fetch_repo() {
    local repo="$1" dir="$2"
    [ -d "$SDK/$dir/.git" ] && return 0
    echo "  fetch $repo @ $VST3_SDK_TAG"
    mkdir -p "$SDK"
    git clone --depth 1 --branch "$VST3_SDK_TAG" -q \
        "https://github.com/steinbergmedia/$repo.git" "$SDK/$dir"
}
fetch_repo vst3_pluginterfaces pluginterfaces
fetch_repo vst3_base base
fetch_repo vst3_public_sdk public.sdk

SOURCES=(
    pluginterfaces/base/conststringtable.cpp
    pluginterfaces/base/coreiids.cpp
    pluginterfaces/base/funknown.cpp
    pluginterfaces/base/ustring.cpp

    base/source/baseiids.cpp
    base/source/fbuffer.cpp
    base/source/fdebug.cpp
    base/source/fdynlib.cpp
    base/source/fobject.cpp
    base/source/fstreamer.cpp
    base/source/fstring.cpp
    base/source/timer.cpp
    base/source/updatehandler.cpp
    base/thread/source/fcondition.cpp
    base/thread/source/flock.cpp

    public.sdk/source/common/commoniids.cpp
    public.sdk/source/common/commonstringconvert.cpp
    public.sdk/source/common/memorystream.cpp
    public.sdk/source/common/pluginview.cpp
    public.sdk/source/common/threadchecker_mac.mm

    public.sdk/source/vst/vstinitiids.cpp
    public.sdk/source/vst/utility/stringconvert.cpp
    public.sdk/source/vst/utility/systemtime.cpp

    public.sdk/source/vst/hosting/connectionproxy.cpp
    public.sdk/source/vst/hosting/eventlist.cpp
    public.sdk/source/vst/hosting/hostclasses.cpp
    public.sdk/source/vst/hosting/hostdataexchangehandler.cpp
    public.sdk/source/vst/hosting/module.cpp
    public.sdk/source/vst/hosting/module_mac.mm
    public.sdk/source/vst/hosting/parameterchanges.cpp
    public.sdk/source/vst/hosting/pluginterfacesupport.cpp
    public.sdk/source/vst/hosting/plugprovider.cpp
    public.sdk/source/vst/hosting/processdata.cpp
)

COMMON_FLAGS=(
    -std=c++17
    -stdlib=libc++
    -O2
    -g
    -fvisibility=hidden
    -fno-common
    -Wno-multichar
    -Wno-deprecated-declarations
    -Wno-unused-parameter
    -DRELEASE=1
    -DDEVELOPMENT=0
    -DSMTG_RENAME_ASSERT=1
    "-I$SDK"
    "-mmacosx-version-min=$DEPLOYMENT_TARGET"
)

SLICES=()
for arch in $ARCHS; do
    objdir="$OUT/$arch"
    mkdir -p "$objdir"
    objects=()
    for src in "${SOURCES[@]}"; do
        obj="$objdir/$(echo "$src" | tr '/.' '__').o"
        objects+=("$obj")
        # Header changes are rare here and dependency tracking would need -MMD
        # plumbing; delete build/vst3sdk to force a full rebuild after an SDK bump.
        if [ -f "$obj" ] && [ "$obj" -nt "$SDK/$src" ]; then
            continue
        fi
        echo "  compile ($arch) $src"
        extra=()
        case "$src" in
            *.mm) extra=(-fobjc-arc -fobjc-weak) ;;
        esac
        # ${extra[@]+...} keeps bash 3.2 from tripping over an empty array under set -u
        xcrun clang++ -arch "$arch" -c "$SDK/$src" -o "$obj" \
            "${COMMON_FLAGS[@]}" ${extra[@]+"${extra[@]}"}
    done

    slice="$objdir/libvst3sdk-$arch.a"
    xcrun libtool -static -no_warning_for_no_symbols -o "$slice" "${objects[@]}"
    SLICES+=("$slice")
done

xcrun lipo -create "${SLICES[@]}" -output "$LIB"
echo "built $LIB"
xcrun lipo -info "$LIB"
