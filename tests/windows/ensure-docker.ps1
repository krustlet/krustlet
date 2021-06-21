# If Docker isn't started, start it
Get-Process 'Docker Desktop' -ErrorVariable NotRunning -ErrorAction SilentlyContinue 2>$null 1>$null

if ($NotRunning) {
    Write-Host "Docker process is not running, starting process"
    Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe"
}
else {
    Write-Host "Docker process is running"
}

# Even if it is running, make sure it is responding
$startTime = Get-Date
Write-Host "Ensuring that Docker is ready for commands"
Do {
    $ErrorActionPreference = 'SilentlyContinue'
    docker ps 2>$null 1>$null
    $exitCode = $LASTEXITCODE
    Write-Host "Docker is not ready, will retry in 5s"
    Start-Sleep 5
} Until ($exitCode -eq 0 -or (Get-Date) -gt $startTime.AddMinutes(3))

if ((Get-Date) -gt $startTime.AddMinutes(3)) {
    Write-Error "Docker never went ready" -ErrorAction Stop
}

Write-Host "Docker is now ready"
