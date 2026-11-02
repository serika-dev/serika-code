# Drive `serika ipc` the way the Qt desktop does and print what comes back.
#
#   .\scripts\ipc-probe.ps1 -Requests '{"id":1,"method":"settings.get"}','{"id":2,"method":"providers.status"}' -Seconds 20
#
# Long fields (screenshots) are shortened so the output stays readable.
param(
    [string[]]$Requests,
    [int]$Seconds = 15,
    [string]$Cwd = (Get-Location).Path,
    [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) "target\debug\serika.exe"),
    [string]$StopOn = ""
)

$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = $Exe
$info.Arguments = "ipc"
$info.WorkingDirectory = $Cwd
$info.RedirectStandardInput = $true
$info.RedirectStandardOutput = $true
$info.RedirectStandardError = $true
$info.UseShellExecute = $false
$info.CreateNoWindow = $true
$proc = [System.Diagnostics.Process]::Start($info)

foreach ($line in $Requests) { $proc.StandardInput.WriteLine($line) }
$proc.StandardInput.Flush()

$deadline = (Get-Date).AddSeconds($Seconds)
$task = $null
while ((Get-Date) -lt $deadline -and -not $proc.HasExited) {
    if ($null -eq $task) { $task = $proc.StandardOutput.ReadLineAsync() }
    if ($task.Wait(250)) {
        $line = $task.Result
        $task = $null
        if ($null -eq $line) { break }
        $short = [regex]::Replace($line, '"image":"[^"]{80,}"', '"image":"<base64 omitted>"')
        if ($short.Length -gt 700) { $short = $short.Substring(0, 700) + " …" }
        $short
        if ($StopOn -and $line -match $StopOn) { break }
    }
}
$proc.StandardInput.Close()
if (-not $proc.WaitForExit(4000)) { $proc.Kill() }
$err = $proc.StandardError.ReadToEnd()
if ($err.Trim()) { "--- stderr"; $err.Trim() }
