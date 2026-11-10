# For scripts/smoke-connect.sh: finds the hearthd that started the
# background service with process id -DaemonPid and tells whether it shows
# its tray window. Prints "<hearthd pid> tray" or "<hearthd pid> none".
# ASCII only: Windows PowerShell 5.1 reads scripts without BOM as ANSI.
# EnumWindows rather than FindWindowEx: PowerShell passes $null for a string
# as "", which would only match windows without a title.
param([int]$DaemonPid)

$parent = (Get-CimInstance Win32_Process -Filter "ProcessId=$DaemonPid").ParentProcessId
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class TrayWindows {
    delegate bool EnumProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")]
    static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassName(IntPtr window, StringBuilder name, int max);
    public static bool Shown(uint owner) {
        bool found = false;
        EnumWindows((window, parameter) => {
            uint processId;
            GetWindowThreadProcessId(window, out processId);
            if (processId == owner) {
                var name = new StringBuilder(256);
                GetClassName(window, name, name.Capacity);
                if (name.ToString() == "hearth-tray") { found = true; }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@
if ([TrayWindows]::Shown([uint32]$parent)) { Write-Output "$parent tray" } else { Write-Output "$parent none" }
