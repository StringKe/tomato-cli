# tomato-cli 安装脚本（Windows）
# irm https://raw.githubusercontent.com/StringKe/tomato-cli/main/scripts/install.ps1 | iex
$ErrorActionPreference = "Stop"
$repo = "StringKe/tomato-cli"
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLower()
switch ($arch) {
    "x64" { $target = "x86_64-pc-windows-msvc" }
    "arm64" { $target = "aarch64-pc-windows-msvc" }
    default { throw "不支持的架构: $arch" }
}

$dir = Join-Path $env:LOCALAPPDATA "tomato-cli"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$url = "https://github.com/$repo/releases/latest/download/tomato-$target.zip"
$zip = Join-Path $dir "tomato.zip"
Write-Host "安装 tomato ($target)"
Write-Host "来源 $url"
Invoke-WebRequest -Uri $url -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $dir -Force
Remove-Item $zip -Force
$exe = Join-Path $dir "tomato.exe"
if (-not (Test-Path $exe)) {
    throw "压缩包里没有 tomato.exe"
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($null -eq $userPath) { $userPath = "" }
if ($userPath -notlike "*$dir*") {
    $newPath = if ($userPath.Trim().Length -eq 0) { $dir } else { "$userPath;$dir" }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    $env:Path = "$dir;$env:Path"
}

Write-Host "已安装到 $exe"
& $exe --version
Write-Host "更新：tomato update"
