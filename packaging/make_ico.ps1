Add-Type -AssemblyName System.Drawing

$pngPath = "$PSScriptRoot\zakuro.png"
$icoPath = "$PSScriptRoot\zakuro.ico"

$src = [System.Drawing.Image]::FromFile($pngPath)
$sizes = @(16, 24, 32, 48, 64, 128, 256)

$msList = New-Object System.Collections.Generic.List[System.IO.MemoryStream]
$pngBytesList = New-Object System.Collections.Generic.List[byte[]]

foreach ($size in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.DrawImage($src, 0, 0, $size, $size)
    $g.Dispose()

    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $pngBytesList.Add($ms.ToArray())
    $ms.Dispose()
}
$src.Dispose()

$fs = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter($fs)

# ICONDIR header
$bw.Write([uint16]0) # Reserved
$bw.Write([uint16]1) # Type (1 for icon)
$bw.Write([uint16]$sizes.Count) # Image count

$offset = 6 + ($sizes.Count * 16)

for ($i = 0; $i -lt $sizes.Count; $i++) {
    $size = $sizes[$i]
    $data = $pngBytesList[$i]
    $w = if ($size -ge 256) { [byte]0 } else { [byte]$size }
    $h = if ($size -ge 256) { [byte]0 } else { [byte]$size }

    $bw.Write($w) # Width
    $bw.Write($h) # Height
    $bw.Write([byte]0) # Color count
    $bw.Write([byte]0) # Reserved
    $bw.Write([uint16]1) # Planes
    $bw.Write([uint16]32) # Bit count
    $bw.Write([uint32]$data.Length) # Bytes in resource
    $bw.Write([uint32]$offset) # Image offset

    $offset += $data.Length
}

foreach ($data in $pngBytesList) {
    $bw.Write($data)
}

$bw.Flush()
$bw.Close()
$fs.Close()

Write-Host "Created $icoPath successfully! File size: $((Get-Item $icoPath).Length) bytes"
