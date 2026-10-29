# Linea base PERF-03: carga de pagina con el worker en debug, medida desde fuera (sin tocar codigo).
# Cada pasada: arranque en frio de la app (portada galapagos://dashboard), espera a que el worker
# quede en reposo, escribe la URL en la barra (WM_SETTEXT) y pulsa Enter (WM_KEYDOWN VK_RETURN).
#   startup_s : lanzamiento -> worker en reposo (arranque de Servo)
#   t_title_s : Enter -> el titulo de la ventana pasa al de la pagina (llega TitleChanged)
#   t_quiet_s : Enter -> el worker queda en reposo (< 0.10 s de CPU en 2 s) tras el titulo
#   cpu_s     : CPU del worker entre Enter y t_quiet; commit/ws: picos durante la carga
param(
    [string]$Exe,
    [string[]]$Urls,
    [int]$Runs = 3,
    [int]$TimeoutS = 120,
    [string]$OutCsv
)

Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class U {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, string l);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
}
'@

function Wait-Quiet($proc, [double]$since, $sw, [double]$limit) {
    # Devuelve el instante (s del cronometro) en que empieza una ventana de 2 s con < 0.10 s de CPU
    $samples = New-Object System.Collections.ArrayList
    while ($sw.Elapsed.TotalSeconds -lt $limit) {
        Start-Sleep -Milliseconds 100
        $proc.Refresh()
        if ($proc.HasExited) { return $null }
        $now = $sw.Elapsed.TotalSeconds
        [void]$samples.Add(@($now, $proc.TotalProcessorTime.TotalSeconds, $proc.PrivateMemorySize64, $proc.WorkingSet64))
        $old = $samples | Where-Object { $_[0] -le ($now - 2.0) } | Select-Object -Last 1
        if ($old -and $old[0] -ge $since -and ($samples[$samples.Count - 1][1] - $old[1]) -lt 0.10) {
            return @{ t = $old[0]; cpu = $old[1]; samples = $samples }
        }
    }
    return $null
}

$results = @()
foreach ($url in $Urls) {
    for ($r = 1; $r -le $Runs; $r++) {
        Get-Process galapagos-content, galapagos-browser -ErrorAction SilentlyContinue | Stop-Process -Force
        Start-Sleep -Milliseconds 800
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $app = Start-Process -FilePath $Exe -PassThru
        $worker = $null
        while (-not $worker -and $sw.Elapsed.TotalSeconds -lt 30) {
            Start-Sleep -Milliseconds 100
            $worker = Get-Process galapagos-content -ErrorAction SilentlyContinue | Select-Object -First 1
        }
        $row = [ordered]@{ url = $url; run = $r; startup_s = $null; t_title_s = $null; t_quiet_s = $null
                           cpu_s = $null; peak_commit_mb = $null; peak_ws_mb = $null; title = ''; note = '' }
        $boot = if ($worker) { Wait-Quiet $worker 1.0 $sw 60 } else { $null }
        $app.Refresh()
        $edit = if ($app.MainWindowHandle -ne 0) { [U]::FindWindowExW($app.MainWindowHandle, [IntPtr]::Zero, 'Edit', [NullString]::Value) } else { [IntPtr]::Zero }
        if (-not $boot -or $edit -eq [IntPtr]::Zero) {
            $row.note = 'sin worker en reposo o sin barra'
        } else {
            $row.startup_s = [math]::Round($boot.t, 1)
            $cpu0 = $worker.TotalProcessorTime.TotalSeconds
            [void][U]::SendMessageW($edit, 0x000C, [IntPtr]::Zero, $url)       # WM_SETTEXT
            [void][U]::PostMessageW($edit, 0x0100, [IntPtr]0x0D, [IntPtr]::Zero) # WM_KEYDOWN VK_RETURN
            $t0 = $sw.Elapsed.TotalSeconds
            while ($sw.Elapsed.TotalSeconds - $t0 -lt $TimeoutS -and $null -eq $row.t_title_s) {
                Start-Sleep -Milliseconds 100
                $app.Refresh()
                if ($app.HasExited) { $row.note = 'la app termino'; break }
                $t = $app.MainWindowTitle
                if ($t -match 'Galapagos Browser$' -and $t -notmatch '^(Dashboard|Galapagos Browser)' -and $t -notmatch 'Trusted Coordinator') {
                    $row.t_title_s = [math]::Round($sw.Elapsed.TotalSeconds - $t0, 1); $row.title = $t
                }
            }
            if ($null -eq $row.t_title_s -and -not $row.note) { $row.note = "sin titulo en $TimeoutS s" }
            $since = if ($null -ne $row.t_title_s) { $t0 + $row.t_title_s } else { $t0 }
            $load = Wait-Quiet $worker $since $sw ($t0 + $TimeoutS + 30)
            if ($load) {
                $row.t_quiet_s = [math]::Round($load.t - $t0, 1)
                $row.cpu_s = [math]::Round($load.cpu - $cpu0, 1)
                $row.peak_commit_mb = [math]::Round((($load.samples | ForEach-Object { $_[2] }) | Measure-Object -Maximum).Maximum / 1MB)
                $row.peak_ws_mb = [math]::Round((($load.samples | ForEach-Object { $_[3] }) | Measure-Object -Maximum).Maximum / 1MB)
            } elseif ($worker.HasExited) {
                $row.note = ($row.note + " worker termino (codigo $($worker.ExitCode))").Trim()
            }
        }
        Get-Process galapagos-content, galapagos-browser -ErrorAction SilentlyContinue | Stop-Process -Force
        $obj = [pscustomobject]$row
        $results += $obj
        ($obj | ConvertTo-Json -Compress) | Write-Output
    }
}
$results | Export-Csv -Path $OutCsv -NoTypeInformation -Encoding UTF8
