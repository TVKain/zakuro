param(
    [Parameter(Mandatory)][string]$Name,      # process name, e.g. zakuro or zakuro-setup
    [Parameter(Mandatory)][string[]]$Steps,   # padctl commands, "| shot" to capture after one
    [Parameter(Mandatory)][string]$Prefix,    # screenshot path prefix
    [string]$PathLike = "*",
    [string]$Padctl = "",
    [int]$Settle = 1500
)
# drives a window with a virtual Xbox 360 pad (ViGEmBus, through padctl):
# brings the window to the front first, since Windows.Gaming.Input, which
# gilrs reads, only gives a program controller input while it is in front.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Drive {
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public static IntPtr Biggest(uint pid) {
    IntPtr best = IntPtr.Zero; long area = 0;
    EnumWindows((h, p) => { uint q; GetWindowThreadProcessId(h, out q);
      if (q == pid && IsWindowVisible(h)) { RECT r; GetWindowRect(h, out r); long a = (long)(r.R - r.L) * (r.B - r.T); if (a > area) { area = a; best = h; } }
      return true; }, IntPtr.Zero);
    return best;
  }
  // past the foreground lock: this thread shares the input state of the
  // window in front, so the switch counts as its own
  public static bool Raise(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    uint none; uint me = GetCurrentThreadId();
    uint front = GetWindowThreadProcessId(GetForegroundWindow(), out none);
    uint target = GetWindowThreadProcessId(h, out none);
    AttachThreadInput(me, front, true); AttachThreadInput(me, target, true);
    if (IsIconic(h)) ShowWindow(h, 9);
    BringWindowToTop(h); SetForegroundWindow(h);
    AttachThreadInput(me, target, false); AttachThreadInput(me, front, false);
    return GetForegroundWindow() == h;
  }
}
"@
if ($Padctl -eq "") { $Padctl = Join-Path $PSScriptRoot "padctl\target\release\padctl.exe" }
function Window {
    $p = Get-Process -Name $Name -ErrorAction SilentlyContinue | Where-Object { $_.Path -like $PathLike } | Sort-Object StartTime -Descending | Select-Object -First 1
    if ($p) { [Drive]::Biggest([uint32]$p.Id) } else { [IntPtr]::Zero }
}
function Raise($h) {
    for ($try = 0; $try -lt 5; $try++) { if ([Drive]::Raise($h)) { return $true }; Start-Sleep -Milliseconds 200 }
    $false
}
function Shot($h, $path) {
    $r = New-Object Drive+RECT; [Drive]::GetWindowRect($h, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc()
    [Drive]::PrintWindow($h, $dc, 2) | Out-Null; $g.ReleaseHdc($dc); $g.Dispose()
    # big windows are saved smaller
    $scale = [math]::Max(1.0, $w / 1200.0)
    $small = New-Object System.Drawing.Bitmap $bmp, ([int]($w / $scale)), ([int]($hh / $scale))
    $small.Save($path); $bmp.Dispose(); $small.Dispose(); $path
}

$h = Window
if ($h -eq [IntPtr]::Zero) { throw "no window of $Name" }
if (-not (Raise $h)) { throw "Windows wouldn't bring $Name to the front" }

$psi = New-Object Diagnostics.ProcessStartInfo $Padctl
$psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
$pc = [Diagnostics.Process]::Start($psi)
$first = $pc.StandardOutput.ReadLine()
if ($first -ne "ready") { throw "padctl failed: $first $($pc.StandardError.ReadToEnd())" }
# the program sees the pad arrive
Start-Sleep -Milliseconds $Settle
$i = 0
try {
    foreach ($step in $Steps) {
        $cmd, $shot = $step.Split("|") | ForEach-Object { $_.Trim() }
        # a step can close the window, or the program can open another
        $now = Window
        if ($now -ne [IntPtr]::Zero) { $h = $now; if ($cmd -and -not (Raise $h)) { Write-Warning "$Name lost the front before '$cmd'" } }
        if ($cmd) { $pc.StandardInput.WriteLine($cmd); $null = $pc.StandardOutput.ReadLine() }
        if ($shot) {
            Start-Sleep -Milliseconds 250
            if ((Window) -ne [IntPtr]::Zero) { Shot $h ("{0}_{1:00}_{2}.png" -f $Prefix, $i, $shot) } else { "$($i): $Name closed" }
        }
        $i++
    }
} finally {
    $pc.StandardInput.WriteLine("quit"); $pc.WaitForExit(3000) | Out-Null
}
