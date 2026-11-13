[CmdletBinding()]
param(
    [string]$HostName = 'gitlab.com',
    [string]$UserName = 'oauth2',
    [string]$ProjectPath = 'D:\Projects\PHPProjects\open-po',
    [string]$GitPath = 'C:\laragon\bin\git\cmd\git.exe'
)

$ErrorActionPreference = 'Stop'
$env:GCM_CREDENTIAL_STORE = 'dpapi'
$env:GCM_INTERACTIVE = 'never'
$env:GIT_TERMINAL_PROMPT = '0'

if (-not (Test-Path -LiteralPath $GitPath -PathType Leaf)) {
    Write-Host "Git tidak ditemukan: $GitPath"
    exit 1
}
if (-not (Test-Path -LiteralPath $ProjectPath -PathType Container)) {
    Write-Host "Folder project tidak ditemukan: $ProjectPath"
    exit 1
}

$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
Write-Host ('User   : {0}' -f $identity)
Write-Host ('Server : {0}' -f $env:COMPUTERNAME)
Write-Host "Menyimpan kredensial Git untuk $HostName (akun $UserName)."
Write-Host 'Jalankan helper ini di server 160 sebagai EGOV22, bukan di PC 143.'
Write-Host 'Gunakan Personal Access Token dengan akses read_repository.'
if ($identity -notmatch '(^|\\)EGOV22$') {
    Write-Host 'PERINGATAN: updater SSH berjalan sebagai EGOV22; kredensial akun ini tidak akan dipakai.'
}
$secure = Read-Host -AsSecureString 'Token'
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try {
    $token = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    $lines = @(
        "protocol=https",
        "host=$HostName",
        "username=$UserName",
        "password=$token",
        ""
    )
    $lines | & $GitPath credential approve
    if ($LASTEXITCODE -ne 0) {
        Write-Host 'Gagal menyimpan kredensial.'
        exit 1
    }
} finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    Remove-Variable token -ErrorAction SilentlyContinue
}

Push-Location $ProjectPath
try {
    $result = @(& $GitPath ls-remote --heads origin 2>&1)
    $code = $LASTEXITCODE
} finally {
    Pop-Location
}

if ($code -ne 0) {
    Write-Host 'Kredensial tersimpan, tetapi verifikasi remote gagal:'
    $result | ForEach-Object { Write-Host ([string]$_) }
    exit 1
}

Write-Host 'Kredensial berhasil disimpan dan remote bisa diakses.'
exit 0
