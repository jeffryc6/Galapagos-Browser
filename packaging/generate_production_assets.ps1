# packaging/generate_production_assets.ps1
# Generacion de recursos graficos oficiales de produccion para Galapagos Browser
# Conforme a ZNVE v2.4.2 (Zero Mocks, Assets Oficiales de Alta Fidelidad)

param (
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"

if (-not $OutputDir) {
    $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    $OutputDir = Join-Path $scriptDir "assets"
}

if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

[void][System.Reflection.Assembly]::LoadWithPartialName("System.Drawing")

function New-GalapagosAsset {
    param (
        [string]$FilePath,
        [int]$W,
        [int]$H,
        [string]$Title = "",
        [string]$Subtitle = "",
        [bool]$IsWide = $false
    )

    $bmp = [System.Drawing.Bitmap]::new($W, $H, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit

    # Fondo degradado Slate 950 -> Slate 900
    $rect = [System.Drawing.Rectangle]::new(0, 0, $W, $H)
    $cBg1 = [System.Drawing.Color]::FromArgb(255, 11, 15, 25)
    $cBg2 = [System.Drawing.Color]::FromArgb(255, 15, 23, 42)
    $brushBg = [System.Drawing.Drawing2D.LinearGradientBrush]::new($rect, $cBg1, $cBg2, 45.0)
    $g.FillRectangle($brushBg, $rect)
    $brushBg.Dispose()

    # Geometria del Escudo Galapagos
    $size = [Math]::Min($W, $H)
    $cx = [int]($W / 2)
    $cy = [int]($H / 2)
    if ($IsWide) {
        $cx = [int]($H * 0.55)
    }

    $radius = [int]($size * 0.38)
    if ($W -le 50) { $radius = [int]($size * 0.42) }

    # Resplandor exterior (Sky 500 translucido)
    $glowColor = [System.Drawing.Color]::FromArgb(70, 14, 165, 233)
    $glowWidth = [float][Math]::Max(2.0, ($size * 0.08))
    $penGlow = [System.Drawing.Pen]::new($glowColor, $glowWidth)
    $g.DrawEllipse($penGlow, [float]($cx - $radius), [float]($cy - $radius), [float]($radius * 2), [float]($radius * 2))
    $penGlow.Dispose()

    # Borde exterior de precision (Sky 500 solido)
    $outerColor = [System.Drawing.Color]::FromArgb(255, 14, 165, 233)
    $outerWidth = [float][Math]::Max(1.5, ($size * 0.035))
    $penOuter = [System.Drawing.Pen]::new($outerColor, $outerWidth)
    $g.DrawEllipse($penOuter, [float]($cx - $radius), [float]($cy - $radius), [float]($radius * 2), [float]($radius * 2))
    $penOuter.Dispose()

    # Capa interior degradada
    $innerRad = [int]($radius * 0.82)
    if ($innerRad -gt 2) {
        $rectInner = [System.Drawing.Rectangle]::new(($cx - $innerRad), ($cy - $innerRad), ($innerRad * 2), ($innerRad * 2))
        $cIn1 = [System.Drawing.Color]::FromArgb(255, 2, 132, 199)
        $cIn2 = [System.Drawing.Color]::FromArgb(255, 12, 74, 110)
        $brushInner = [System.Drawing.Drawing2D.LinearGradientBrush]::new($rectInner, $cIn1, $cIn2, 90.0)
        $g.FillEllipse($brushInner, $rectInner)
        $brushInner.Dispose()
    }

    # Nucleo de titanio (Ocean Core Shield)
    $coreRad = [int]($radius * 0.44)
    if ($coreRad -gt 1) {
        $brushCore = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 240, 249, 255))
        $g.FillEllipse($brushCore, ($cx - $coreRad), ($cy - $coreRad), ($coreRad * 2), ($coreRad * 2))
        $brushCore.Dispose()
    }

    # Tipografia oficial en banners anchos
    if ($Title -and $IsWide) {
        $fontTitle = [System.Drawing.Font]::new("Segoe UI Semibold", [float][Math]::Max(13.0, ($H * 0.13)), [System.Drawing.FontStyle]::Bold)
        $brushText = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 248, 250, 252))
        $textX = [float]($H * 1.05)
        $textY = [float]($cy - ($H * 0.17))
        $g.DrawString($Title, $fontTitle, $brushText, $textX, $textY)

        if ($Subtitle) {
            $fontSub = [System.Drawing.Font]::new("Segoe UI", [float][Math]::Max(9.0, ($H * 0.075)), [System.Drawing.FontStyle]::Regular)
            $brushSub = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 148, 163, 184))
            $g.DrawString($Subtitle, $fontSub, $brushSub, $textX, [float]($textY + ($H * 0.22)))
            $fontSub.Dispose()
            $brushSub.Dispose()
        }
        $fontTitle.Dispose()
        $brushText.Dispose()
    }

    $g.Dispose()
    $bmp.Save($FilePath, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

Write-Host "Generando recursos graficos de produccion en $OutputDir..." -ForegroundColor Cyan

New-GalapagosAsset -FilePath (Join-Path $OutputDir "StoreLogo.png") -W 50 -H 50
New-GalapagosAsset -FilePath (Join-Path $OutputDir "Square44x44Logo.png") -W 44 -H 44
New-GalapagosAsset -FilePath (Join-Path $OutputDir "Square150x150Logo.png") -W 150 -H 150
New-GalapagosAsset -FilePath (Join-Path $OutputDir "Wide310x150Logo.png") -W 310 -H 150 -Title "GALAPAGOS" -Subtitle "SECURE BROWSER v0.1" -IsWide $true
New-GalapagosAsset -FilePath (Join-Path $OutputDir "SplashScreen.png") -W 620 -H 300 -Title "GALAPAGOS BROWSER" -Subtitle "Zero-Noise Web Architecture · ZNVE v2.4.2" -IsWide $true

Write-Host "Assets de produccion generados exitosamente." -ForegroundColor Green
