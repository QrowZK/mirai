# Stages the GStreamer runtime DLLs required by a media-enabled Mirai build
# into a directory that the installer bundles alongside mirai.exe.
#
# libservo (components/servo/servo.rs) loads GStreamer plugins from the
# directory containing the executable on Windows, so everything is staged flat.
#
# The DLL and plugin lists mirror Servo's python/servo/gstreamer.py and
# components/servo/gstreamer_plugin_lists/{common,windows}.rs.in at our pinned
# Servo revision. Keep them in sync when bumping the Servo pin.

param(
    [string]$GStreamerRoot = "C:\gstreamer\1.0\msvc_x86_64",
    [string]$OutDir = "target\release\gst-dlls"
)

$ErrorActionPreference = "Stop"

$baseLibs = @(
    "gstbase", "gstcontroller", "gstnet", "gstreamer",
    "gstapp", "gstaudio", "gstfft", "gstgl", "gstpbutils", "gstplay",
    "gstriff", "gstrtp", "gstrtsp", "gstsctp", "gstsdp", "gsttag", "gstvideo",
    "gstcodecparsers", "gstplayer", "gstwebrtc", "gstwebrtcnice"
)

$dependencyDlls = @(
    "avcodec-59.dll", "avfilter-8.dll", "avformat-59.dll", "avutil-57.dll",
    "bz2.dll", "ffi-7.dll", "gio-2.0-0.dll", "glib-2.0-0.dll",
    "gmodule-2.0-0.dll", "gobject-2.0-0.dll", "graphene-1.0-0.dll",
    "intl-8.dll", "libcrypto-1_1-x64.dll", "libjpeg-8.dll", "libogg-0.dll",
    "libpng16-16.dll", "libssl-1_1-x64.dll", "libvorbis-0.dll",
    "libvorbisenc-2.dll", "libwinpthread-1.dll", "nice-10.dll", "opus-0.dll",
    "orc-0.4-0.dll", "pcre2-8-0.dll", "swresample-4.dll", "theora-0.dll",
    "theoradec-1.dll", "theoraenc-1.dll", "z-1.dll"
)

$plugins = @(
    "gstcoreelements", "gstnice", "gstapp", "gstaudioconvert",
    "gstaudioresample", "gstgio", "gstogg", "gstopengl", "gstopus",
    "gstplayback", "gsttheora", "gsttypefindfunctions",
    "gstvideoconvertscale", "gstvolume", "gstvorbis", "gstaudiofx",
    "gstaudioparsers", "gstautodetect", "gstdeinterlace", "gstid3demux",
    "gstinterleave", "gstisomp4", "gstmatroska", "gstrtp", "gstrtpmanager",
    "gstvideofilter", "gstvpx", "gstwavparse", "gstaudiobuffersplit",
    "gstdtls", "gstid3tag", "gstproxy", "gstvideoparsersbad", "gstwebrtc",
    "gstlibav", "gstwasapi"
)

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$missing = @()
foreach ($dll in ($dependencyDlls + ($baseLibs | ForEach-Object { "$_-1.0-0.dll" }))) {
    $source = Join-Path "$GStreamerRoot\bin" $dll
    if (Test-Path $source) {
        Copy-Item $source $OutDir
    } else {
        $missing += $dll
    }
}
foreach ($plugin in ($plugins | ForEach-Object { "$_.dll" })) {
    $source = Join-Path "$GStreamerRoot\lib\gstreamer-1.0" $plugin
    if (Test-Path $source) {
        Copy-Item $source $OutDir
    } else {
        $missing += $plugin
    }
}

if ($missing.Count -gt 0) {
    Write-Error "Missing GStreamer files: $($missing -join ', ')"
}
$count = (Get-ChildItem $OutDir).Count
Write-Host "Staged $count GStreamer DLLs into $OutDir"
