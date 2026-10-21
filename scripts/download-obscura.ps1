# 下载 Obscura sidecar 二进制（Windows 版）
#
# 用法（PowerShell）:
#   .\scripts\download-obscura.ps1
#
# 下载完成后二进制位于 src-tauri/binaries/obscura-x86_64-pc-windows-msvc.exe
# Tauri 的 externalBin 配置会自动识别带目标三元组的文件。

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir = Split-Path -Parent $ScriptDir
$BinDir = Join-Path $ProjectDir "src-tauri\binaries"
$ReleaseBase = "https://github.com/h4ckf0r0day/obscura/releases/latest/download"

$Triple = "x86_64-pc-windows-msvc"
$Asset = "obscura-x86_64-windows.zip"
$Target = Join-Path $BinDir "obscura-$Triple.exe"

if (Test-Path $Target) {
    Write-Host "已存在: $Target，跳过下载"
    exit 0
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
$TmpAsset = Join-Path $BinDir $Asset

Write-Host "下载 Obscura ($Asset) ..."
Invoke-WebRequest -Uri "$ReleaseBase/$Asset" -OutFile $TmpAsset -UseBasicParsing

Write-Host "解压 ..."
Expand-Archive -Force -LiteralPath $TmpAsset -DestinationPath $BinDir

# 按 Tauri sidecar 约定重命名（Windows 加 .exe 后缀）
$ObscuraExe = Join-Path $BinDir "obscura.exe"
if (Test-Path $ObscuraExe) {
    Move-Item -Force $ObscuraExe $Target
}

# worker 仅 scrape 命令需要，serve 模式用不到
$WorkerExe = Join-Path $BinDir "obscura-worker.exe"
if (Test-Path $WorkerExe) {
    Move-Item -Force $WorkerExe (Join-Path $BinDir "obscura-worker-$Triple.exe")
}

Remove-Item -Force $TmpAsset
Write-Host "完成: $Target"
& $Target --version
