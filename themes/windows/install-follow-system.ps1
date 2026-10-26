<#
.SYNOPSIS
Registers (or with -Remove, unregisters) the "Eggshell Follow System" scheduled task,
which runs follow-system.vbs at logon and every minute, then runs it once now.
#>
param([switch]$Remove)

$name = 'Eggshell Follow System'
$vbs = Join-Path $PSScriptRoot 'follow-system.vbs'

if ($Remove) {
    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
    return
}

$action = New-ScheduledTaskAction -Execute 'wscript.exe' -Argument "//B //Nologo `"$vbs`""
$triggers = @(
    New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
    New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 1)
)
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -StartWhenAvailable -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName $name -Action $action -Trigger $triggers -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $name
