# Arnes de caracterizacion COMPAT (Golden Master) del worker Servo.
# No modifica codigo de produccion: sirve sondas locales, las carga en el navegador real con
# packaging\measure_load.ps1 y compara lo que el worker deja en worker_stderr.log.
#
#   Verificar:  powershell -File tests\characterization\compat\Run-CompatHarness.ps1
#   Capturar:   ... -Mode Capture             (solo si no existe el snapshot)
#   Recapturar: ... -Mode Capture -Approve    (requiere aprobacion del Director)
#   Sitios:     ... -Mode Sites               (sitios reales: metricas, no se comparan)
#
# AVISO: measure_load.ps1 termina cualquier galapagos-browser/galapagos-content abierto.
param(
    [ValidateSet('Verify', 'Capture', 'Sites')]
    [string]$Mode = 'Verify',
    [switch]$Approve,
    [string[]]$Probes = @('caps', 'layout', 'errors', 'popups', 'crash'),
    [string[]]$Sites = @('https://www.google.com/', 'https://es.wikipedia.org/wiki/Islas_Gal%C3%A1pagos', 'https://servo.org/', 'https://www.youtube.com/'),
    [int]$Port = 8765,
    [int]$TimeoutS = 40
)
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = (Resolve-Path (Join-Path $here '..\..\..')).Path
$exe = Join-Path $root 'target\debug\galapagos-browser.exe'
$measure = Join-Path $root 'packaging\measure_load.ps1'
$probeDir = Join-Path $here 'probes'
$snapDir = Join-Path $here 'snapshots'
$runDir = Join-Path $here 'runs'
$logDir = Join-Path $env:LOCALAPPDATA 'GalapagosBrowser\logs'
$workerLog = Join-Path $logDir 'worker_stderr.log'
$traceLog = Join-Path $logDir 'app_trace.log'
New-Item -ItemType Directory -Force $snapDir, $runDir | Out-Null

# Lineas del worker que forman el Golden Master (telemetria por excepcion de DELTA_10)
$keep = '^\[(probe:[a-z-]+|console:error|control:sin-soporte|aux:rechazado|content:crash)\]'

function Get-Length($path) { if (Test-Path $path) { (Get-Item $path).Length } else { 0 } }

# Bytes nuevos desde $offset (si el archivo se trunco al superar su tope, se lee entero)
function Read-NewLines($path, [long]$offset) {
    if (-not (Test-Path $path)) { return @() }
    $fs = [System.IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
    try {
        if ($fs.Length -lt $offset) { $offset = 0 }
        [void]$fs.Seek($offset, 'Begin')
        $reader = New-Object System.IO.StreamReader($fs, [System.Text.Encoding]::UTF8)
        $text = $reader.ReadToEnd()
    } finally { $fs.Dispose() }
    return @($text -split "`r?`n" | Where-Object { $_ -ne '' })
}

# Mascara de campos volatiles: el puerto local (las sondas no emiten tiempos, PIDs ni idioma)
function Format-Line([string]$line) {
    return ($line -replace "localhost:$Port", 'localhost:PORT')
}

function Invoke-Load([string]$url) {
    $csv = Join-Path $runDir 'last_load.csv'
    if (Test-Path $csv) { Remove-Item $csv }
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $measure -Exe $exe -Urls $url -Runs 1 -TimeoutS $TimeoutS -OutCsv $csv 2>&1
    if (-not (Test-Path $csv)) { throw "measure_load.ps1 no produjo CSV para $url`n$out" }
    return (Import-Csv $csv | Select-Object -First 1)
}

if (-not (Test-Path $exe)) { throw "Falta el binario: $exe" }

if ($Mode -eq 'Sites') {
    $rows = foreach ($s in $Sites) {
        $w0 = Get-Length $workerLog; $t0 = Get-Length $traceLog
        $m = Invoke-Load $s
        $new = Read-NewLines $workerLog $w0
        $faults = @(Read-NewLines $traceLog $t0 | Where-Object { $_ -match 'WORKER_FAULT|CHANNEL_VIOLATION' }).Count
        [pscustomobject]@{
            url = $s; t_title_s = $m.t_title_s; t_quiet_s = $m.t_quiet_s; cpu_s = $m.cpu_s; peak_commit_mb = $m.peak_commit_mb
            console_errors = @($new | Where-Object { $_ -match '^\[console:error\]' }).Count
            controls = @($new | Where-Object { $_ -match '^\[control:sin-soporte\]' }).Count
            worker_faults = $faults; title = $m.title
        }
    }
    $out = Join-Path $runDir ("sites_{0}.csv" -f (Get-Date -Format 'yyyyMMdd_HHmmss'))
    $rows | Export-Csv -NoTypeInformation -Encoding UTF8 $out
    $rows | Format-Table url, t_quiet_s, cpu_s, peak_commit_mb, console_errors, controls, worker_faults -AutoSize | Out-String
    "Metricas en $out"
    exit 0
}

# Servidor local de las sondas (System.Net.HttpListener, sin dependencias)
$server = Start-Job -ArgumentList $probeDir, $Port -ScriptBlock {
    param($dir, $port)
    $l = New-Object System.Net.HttpListener
    $l.Prefixes.Add("http://localhost:$port/")
    $l.Start()
    while ($l.IsListening) {
        $ctx = $l.GetContext()
        $name = [System.IO.Path]::GetFileName($ctx.Request.Url.AbsolutePath)
        $file = Join-Path $dir $name
        if ($name -and (Test-Path $file -PathType Leaf)) {
            $bytes = [System.IO.File]::ReadAllBytes($file)
            $ctx.Response.ContentType = if ($name.EndsWith('.js')) { 'text/javascript; charset=utf-8' } else { 'text/html; charset=utf-8' }
            $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
        } else {
            $ctx.Response.StatusCode = 404
        }
        $ctx.Response.Close()
    }
}

$failures = 0
try {
    # El servidor esta listo cuando responde a la sonda de capacidades (plazo acotado)
    $ready = $false
    for ($i = 0; $i -lt 50 -and -not $ready; $i++) {
        try { $ready = (Invoke-WebRequest -UseBasicParsing "http://localhost:$Port/probe.js" -TimeoutSec 1).StatusCode -eq 200 } catch { Start-Sleep -Milliseconds 100 }
    }
    if (-not $ready) { throw "El servidor de sondas no respondio en http://localhost:$Port/ ($(Receive-Job $server 2>&1))" }

    foreach ($p in $Probes) {
        $w0 = Get-Length $workerLog; $t0 = Get-Length $traceLog
        # DELTA_11 D53: solo la sonda crash recibe el disparador de panico (worker debug); el
        # Coordinador lo hereda en su entorno y lo pasa al worker (CreateProcessW sin entorno propio)
        if ($p -eq 'crash') { $env:GALAPAGOS_TEST_SERVO_HELPERS = '1' }
        try { $m = Invoke-Load "http://localhost:$Port/$p.html" }
        finally { Remove-Item Env:\GALAPAGOS_TEST_SERVO_HELPERS -ErrorAction SilentlyContinue }
        $lines = @(Read-NewLines $workerLog $w0 | Where-Object { $_ -match $keep } | ForEach-Object { Format-Line $_ })
        [Array]::Sort($lines, [System.StringComparer]::Ordinal)
        $trace = @(Read-NewLines $traceLog $t0)
        $faults = @($trace | Where-Object { $_ -match 'WORKER_FAULT|CHANNEL_VIOLATION' }).Count
        $snapshot = @("probe=$p", "title=$($m.title)", "worker_faults=$faults", "lines=$($lines.Count)")
        if ($p -eq 'crash') { $snapshot += "content_crashed=$(@($trace | Where-Object { $_ -match 'CONTENT_CRASHED' }).Count)" }
        $snapshot += $lines
        Set-Content -Encoding UTF8 (Join-Path $runDir "$p.txt") $snapshot

        # El worker antepone `[console:error] <host>`: el marcador puede ir en cualquier posicion
        if (-not ($lines -match "\[probe:done\] $p")) {
            $failures++; "FALLO $p - esperado [probe:done] $p - recibido sonda incompleta (ver runs\$p.txt)"
            continue
        }
        $snap = Join-Path $snapDir "$p.txt"
        if ($Mode -eq 'Capture') {
            if ((Test-Path $snap) -and -not $Approve) {
                $failures++; "FALLO $p - snapshot existente: recapturar requiere -Approve (aprobacion del Director)"
                continue
            }
            Set-Content -Encoding UTF8 $snap $snapshot
            "CAPTURADO $snap ($($lines.Count) lineas)"
            continue
        }
        if (-not (Test-Path $snap)) { $failures++; "FALLO $p - esperado snapshots\$p.txt - recibido inexistente (usa -Mode Capture)"; continue }
        $expected = @(Get-Content -Encoding UTF8 $snap)
        $n = [Math]::Max($expected.Count, $snapshot.Count)
        for ($k = 0; $k -lt $n; $k++) {
            $e = if ($k -lt $expected.Count) { $expected[$k] } else { '<fin>' }
            $r = if ($k -lt $snapshot.Count) { $snapshot[$k] } else { '<fin>' }
            if ($e -cne $r) { $failures++; "FALLO snapshots\$p.txt:$($k + 1) - esperado $e - recibido $r" }
        }
    }
} finally {
    Stop-Job $server -ErrorAction SilentlyContinue
    Remove-Job $server -Force -ErrorAction SilentlyContinue
}
"fallos=$failures"
exit $failures
