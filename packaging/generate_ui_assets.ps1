# packaging/generate_ui_assets.ps1
# DELTA_08 paso 2 (D34): genera el logo y el icono de la interfaz a partir del tablero de marca.
#   assets/galapagos-logo.png  emblema circular 04 recortado, fondo transparente, 128x128
#   assets/galapagos.ico       mismo emblema en 16, 20, 24, 32, 40, 48, 64 y 128 px (DIB de 32 bits)
# Solo usa System.Drawing (incluido en Windows). Se ejecuta a mano al cambiar la marca; los
# archivos generados se versionan en assets/.

param(
    [string]$Board = '',
    [string]$OutDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'assets')
)

$ErrorActionPreference = 'Stop'

# El nombre del tablero lleva tilde; PowerShell 5.1 lee los .ps1 sin BOM como ANSI, así que se
# localiza por patrón ASCII en lugar de escribir el nombre literal.
if (-not $Board) {
    $match = Get-ChildItem (Split-Path -Parent $PSScriptRoot) -Filter '*Logo Concept Board.png' -File | Select-Object -First 1
    if ($match) { $Board = $match.FullName }
}
Add-Type -AssemblyName System.Drawing

# Emblema 04 (variante circular) en el tablero de 1254x1254: centro y radio medidos sobre el original
$CenterX = 316
$CenterY = 845
$Radius  = 70
$IconSizes = @(16, 20, 24, 32, 40, 48, 64, 128)

if (-not (Test-Path $Board)) { throw "No se encuentra el tablero de marca: $Board" }
New-Item -ItemType Directory -Force $OutDir | Out-Null

# Emblema escalado a $Size x $Size con máscara circular suavizada (fuera del círculo: transparente)
function New-Emblem([System.Drawing.Bitmap]$Source, [int]$Size) {
    $side = 2 * $Radius
    $srcRect = [System.Drawing.Rectangle]::new($CenterX - $Radius, $CenterY - $Radius, $side, $side)
    $bmp = [System.Drawing.Bitmap]::new($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $g.Clear([System.Drawing.Color]::Transparent)
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

        $scaled = [System.Drawing.Bitmap]::new($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $gs = [System.Drawing.Graphics]::FromImage($scaled)
        $gs.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $gs.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $gs.DrawImage($Source, [System.Drawing.Rectangle]::new(0, 0, $Size, $Size), $srcRect, [System.Drawing.GraphicsUnit]::Pixel)
        $gs.Dispose()

        $brush = [System.Drawing.TextureBrush]::new($scaled)
        $g.FillEllipse($brush, 0, 0, $Size - 1, $Size - 1)
        $brush.Dispose()
        $scaled.Dispose()
    } finally {
        $g.Dispose()
    }
    return $bmp
}

# Entrada de ICO en formato DIB de 32 bits (BITMAPINFOHEADER + BGRA de abajo arriba + máscara AND).
# Es el formato que entienden todos los lectores de Windows; las entradas PNG no las decodifica
# System.Drawing y salen como ruido.
function Get-DibIconBytes([System.Drawing.Bitmap]$Bitmap) {
    $size = $Bitmap.Width
    $maskStride = [int]([Math]::Ceiling($size / 32.0) * 4)
    $ms = [System.IO.MemoryStream]::new()
    $w = [System.IO.BinaryWriter]::new($ms)
    try {
        $w.Write([uint32]40); $w.Write([int32]$size); $w.Write([int32]($size * 2))
        $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]0)
        $w.Write([uint32]($size * $size * 4 + $maskStride * $size))
        $w.Write([int32]0); $w.Write([int32]0); $w.Write([uint32]0); $w.Write([uint32]0)
        for ($y = $size - 1; $y -ge 0; $y--) {
            for ($x = 0; $x -lt $size; $x++) {
                $c = $Bitmap.GetPixel($x, $y)
                $w.Write([byte]$c.B); $w.Write([byte]$c.G); $w.Write([byte]$c.R); $w.Write([byte]$c.A)
            }
        }
        # Máscara AND a cero: la transparencia la da el canal alfa
        $w.Write([byte[]]::new($maskStride * $size))
        $w.Flush()
        return ,$ms.ToArray()
    } finally {
        $w.Dispose()
        $ms.Dispose()
    }
}

$source = [System.Drawing.Bitmap]::FromFile($Board)
try {
    # 1. Logo para la interfaz
    $logo = New-Emblem $source 128
    $logo.Save((Join-Path $OutDir 'galapagos-logo.png'), [System.Drawing.Imaging.ImageFormat]::Png)
    $logo.Dispose()

    # 2. ICO con entradas DIB de 32 bits
    $images = foreach ($size in $IconSizes) {
        $bmp = New-Emblem $source $size
        [pscustomobject]@{ Size = $size; Bytes = (Get-DibIconBytes $bmp) }
        $bmp.Dispose()
    }
    $fs = [System.IO.File]::Create((Join-Path $OutDir 'galapagos.ico'))
    $w = [System.IO.BinaryWriter]::new($fs)
    try {
        $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$images.Count)
        $offset = 6 + 16 * $images.Count
        foreach ($img in $images) {
            $dim = if ($img.Size -ge 256) { 0 } else { $img.Size }
            $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
            $w.Write([uint16]1); $w.Write([uint16]32)
            $w.Write([uint32]$img.Bytes.Length); $w.Write([uint32]$offset)
            $offset += $img.Bytes.Length
        }
        foreach ($img in $images) { $w.Write($img.Bytes) }
    } finally {
        $w.Dispose()
        $fs.Dispose()
    }
} finally {
    $source.Dispose()
}

Get-ChildItem $OutDir -Include 'galapagos-logo.png', 'galapagos.ico' -File -Recurse | ForEach-Object { "{0}  {1} B" -f $_.Name, $_.Length }
