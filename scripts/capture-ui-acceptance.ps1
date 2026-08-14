param(
    [int]$ProcessId = 0,
    [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$output = if ($OutputDirectory) { $OutputDirectory } else { Join-Path $root 'artifacts' }

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class AlterSendmeUiCaptureNative {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct Point { public int X, Y; }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool GetClientRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool ClientToScreen(IntPtr window, ref Point point);

    [DllImport("user32.dll")]
    public static extern bool ScreenToClient(IntPtr window, ref Point point);

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(
        IntPtr window, IntPtr insertAfter, int x, int y, int width, int height, uint flags);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr window, int command);

    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr window, IntPtr deviceContext, uint flags);

    [DllImport("user32.dll")]
    public static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);

    public static void ClickWindow(IntPtr window, int screenX, int screenY) {
        var point = new Point { X = screenX, Y = screenY };
        ScreenToClient(window, ref point);
        var packed = (point.X & 0xffff) | ((point.Y & 0xffff) << 16);
        SendMessage(window, 0x0200, IntPtr.Zero, (IntPtr)packed);
        SendMessage(window, 0x0201, (IntPtr)1, (IntPtr)packed);
        SendMessage(window, 0x0202, IntPtr.Zero, (IntPtr)packed);
    }
}
'@

$process = if ($ProcessId) {
    Get-Process -Id $ProcessId
} else {
    Get-Process alter-sendme-gpui | Select-Object -First 1
}
$window = $process.MainWindowHandle
if ($window -eq [IntPtr]::Zero) {
    throw 'AlterSendme does not have a visible main window.'
}

New-Item -ItemType Directory -Path $output -Force | Out-Null
$originalCursor = New-Object AlterSendmeUiCaptureNative+Point
[AlterSendmeUiCaptureNative]::GetCursorPos([ref]$originalCursor) | Out-Null

function Get-WindowRoot {
    # Re-read the accessibility tree after every render so element bounds and visibility are current.
    return [System.Windows.Automation.AutomationElement]::FromHandle($window)
}

function Find-Control($Type, [int]$Index = 0) {
    $condition = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        $Type
    )
    $matches = (Get-WindowRoot).FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        $condition
    )
    if ($matches.Count -le $Index) {
        throw "Expected control $Type at index $Index, found $($matches.Count)."
    }
    return $matches.Item($Index)
}

function Get-LanguageOptionCount {
    $condition = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::ListItem
    )
    return (Get-WindowRoot).FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        $condition
    ).Count
}

function Click-Control($Control) {
    # Click the accessibility bounds instead of hard-coded screen coordinates so resized captures stay valid.
    $name = $Control.Current.Name
    $type = $Control.Current.ControlType
    [AlterSendmeUiCaptureNative]::ShowWindow($window, 9) | Out-Null
    [AlterSendmeUiCaptureNative]::SetForegroundWindow($window) | Out-Null
    Start-Sleep -Milliseconds 150
    $conditions = New-Object System.Windows.Automation.AndCondition(
        (New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            $type
        )),
        (New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::NameProperty,
            $name
        ))
    )
    $Control = (Get-WindowRoot).FindFirst(
        [System.Windows.Automation.TreeScope]::Descendants,
        $conditions
    )
    if ($null -eq $Control) {
        throw "Control disappeared before click: $name"
    }
    $bounds = $Control.Current.BoundingRectangle
    $x = [int]($bounds.Left + ($bounds.Width / 2))
    $y = [int]($bounds.Top + ($bounds.Height / 2))
    [AlterSendmeUiCaptureNative]::SetForegroundWindow($window) | Out-Null
    [AlterSendmeUiCaptureNative]::ClickWindow($window, $x, $y)
    Start-Sleep -Milliseconds 350
}

function Resize-Client([int]$Width, [int]$Height) {
    # Win32 resizes the outer frame, so preserve the current non-client border when targeting GPUI client pixels.
    [AlterSendmeUiCaptureNative]::ShowWindow($window, 9) | Out-Null
    $outer = New-Object AlterSendmeUiCaptureNative+Rect
    $client = New-Object AlterSendmeUiCaptureNative+Rect
    [AlterSendmeUiCaptureNative]::GetWindowRect($window, [ref]$outer) | Out-Null
    [AlterSendmeUiCaptureNative]::GetClientRect($window, [ref]$client) | Out-Null
    $frameWidth = ($outer.Right - $outer.Left) - ($client.Right - $client.Left)
    $frameHeight = ($outer.Bottom - $outer.Top) - ($client.Bottom - $client.Top)
    [AlterSendmeUiCaptureNative]::SetWindowPos(
        $window,
        [IntPtr]::Zero,
        0,
        0,
        $Width + $frameWidth,
        $Height + $frameHeight,
        0x0016
    ) | Out-Null
    Start-Sleep -Milliseconds 450
}

function Save-WindowScreenshot([string]$Name) {
    # Ask DWM for the test window surface so screenshots remain valid when another app is foreground.
    [AlterSendmeUiCaptureNative]::ShowWindow($window, 9) | Out-Null
    Start-Sleep -Milliseconds 200
    $rect = New-Object AlterSendmeUiCaptureNative+Rect
    [AlterSendmeUiCaptureNative]::GetWindowRect($window, [ref]$rect) | Out-Null
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $deviceContext = $graphics.GetHdc()
        try {
            $printed = [AlterSendmeUiCaptureNative]::PrintWindow($window, $deviceContext, 2)
        } finally {
            $graphics.ReleaseHdc($deviceContext)
        }
        if (-not $printed) {
            [AlterSendmeUiCaptureNative]::SetWindowPos(
                $window, [IntPtr](-1), 0, 0, 0, 0, 0x0013
            ) | Out-Null
            [AlterSendmeUiCaptureNative]::SetForegroundWindow($window) | Out-Null
            Start-Sleep -Milliseconds 200
            $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
            [AlterSendmeUiCaptureNative]::SetWindowPos(
                $window, [IntPtr](-2), 0, 0, 0, 0, 0x0013
            ) | Out-Null
        }
        $path = Join-Path $output $Name
        $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
    Write-Host "Captured $path"
}

try {
    Resize-Client 1024 720

    $language = Find-Control ([System.Windows.Automation.ControlType]::ComboBox)
    if ((Get-LanguageOptionCount) -gt 0) {
        Click-Control (Find-Control ([System.Windows.Automation.ControlType]::ListItem))
    }

    Save-WindowScreenshot 'acceptance-send-1024x720-current.png'
    Click-Control (Find-Control ([System.Windows.Automation.ControlType]::ComboBox))
    if ((Get-LanguageOptionCount) -ne 21) {
        throw "Language dropdown did not expose all 21 options."
    }
    Save-WindowScreenshot 'acceptance-language-dropdown-current.png'

    Click-Control (Find-Control ([System.Windows.Automation.ControlType]::ListItem))
    if ((Get-LanguageOptionCount) -ne 0) {
        throw 'Language dropdown did not close after reselecting the active language.'
    }

    Click-Control (Find-Control ([System.Windows.Automation.ControlType]::Button) 2)
    Save-WindowScreenshot 'acceptance-settings-1024x720-current.png'
    Click-Control (Find-Control ([System.Windows.Automation.ControlType]::Button) 2)

    Click-Control (Find-Control ([System.Windows.Automation.ControlType]::TabItem) 1)
    Save-WindowScreenshot 'acceptance-receive-1024x720-current.png'

    Click-Control (Find-Control ([System.Windows.Automation.ControlType]::TabItem) 0)
    Resize-Client 760 560
    Save-WindowScreenshot 'acceptance-min-760x560-current.png'
} finally {
    [AlterSendmeUiCaptureNative]::SetCursorPos($originalCursor.X, $originalCursor.Y) | Out-Null
}
