Add-Type -AssemblyName System.Drawing

$size = 1024
$bmp = New-Object System.Drawing.Bitmap($size, $size)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias

# Rounded-rect app tile
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$r = 220
$path.AddArc(0, 0, $r, $r, 180, 90)
$path.AddArc($size - $r, 0, $r, $r, 270, 90)
$path.AddArc($size - $r, $size - $r, $r, $r, 0, 90)
$path.AddArc(0, $size - $r, $r, $r, 90, 90)
$path.CloseFigure()

$bg = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 22, 24, 32))
$g.FillPath($bg, $path)

# Subtle top-light edge
$edge = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(40, 255, 255, 255), 6)
$g.DrawPath($edge, $path)

# Terminal prompt: "> _" in accent blue
$accent = [System.Drawing.Color]::FromArgb(255, 124, 148, 255)
$pen = New-Object System.Drawing.Pen($accent, 72)
$pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
$pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round

# caret ">"
$g.DrawLine($pen, 250, 340, 450, 512)
$g.DrawLine($pen, 450, 512, 250, 684)

# underscore "_"
$g.DrawLine($pen, 540, 684, 790, 684)

$out = Join-Path $PSScriptRoot "icon-source.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()
Write-Host "saved: $out"
