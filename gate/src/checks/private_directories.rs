//! Private temporary directories under the runner's own.

#[cfg(windows)]
use crate::runner::{Cmd, Failure};
#[cfg(unix)]
use std::fs::DirBuilder;
use std::io;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process;
use std::time;
use std::time::SystemTime;

/// Unix creation applies the owner-only mode in the mkdir system call.
#[cfg(unix)]
pub(crate) fn create_private(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

/// Windows creation supplies a protected inheritable DACL atomically and
/// preserves `ERROR_ALREADY_EXISTS`. The fixed adapter passes paths as data.
#[cfg(windows)]
pub(crate) fn create_private(path: &Path) -> io::Result<()> {
    Cmd::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            PRIVATE_DIRECTORY,
        ])
        .env("RUST_GATE_PRIVATE_DIRECTORY", path)
        .run()
        .map_err(private_creation_error)
}

/// Preserve both Windows collision statuses, and relay every other failure.
#[cfg(windows)]
fn private_creation_error(error: Failure) -> io::Error {
    if error.code == 183 || error.code == 80 {
        io::Error::from(ErrorKind::AlreadyExists)
    } else {
        io::Error::other(
            error.message.unwrap_or_else(|| {
                format!("Windows private directory creation exited {}", error.code)
            }),
        )
    }
}

/// .NET's directory creation accepts existing directories, so the minimal
/// interop adapter calls `CreateDirectoryW` directly with `SECURITY_ATTRIBUTES`.
/// The descriptor is always freed, even on collision or an OS failure.
#[cfg(windows)]
const PRIVATE_DIRECTORY: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using Microsoft.Win32.SafeHandles;
public static class PrivateDirectory {
    [StructLayout(LayoutKind.Sequential)]
    private struct Attributes {
        public int Length;
        public IntPtr Descriptor;
        public int Inherit;
    }
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptorW(
        string text, uint revision, out IntPtr descriptor, out uint size);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateDirectoryW(string path, ref Attributes attributes);
    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr pointer);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFileW(string path, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool GetVolumeInformationByHandleW(SafeFileHandle handle,
        IntPtr name, uint nameLength, out uint serial, out uint componentLength,
        out uint flags, IntPtr fileSystem, uint fileSystemLength);
    private static int CheckVolume(string path) {
        // Follow the actual directory target, not a lexical drive-letter guess.
        using (var handle = CreateFileW(path, 0, 7, IntPtr.Zero, 3, 0x02000000, IntPtr.Zero)) {
            if (handle.IsInvalid) {
                int error = Marshal.GetLastWin32Error();
                return error == 2 ? 3 : error;
            }
            uint serial, componentLength, flags;
            if (!GetVolumeInformationByHandleW(handle, IntPtr.Zero, 0, out serial,
                out componentLength, out flags, IntPtr.Zero, 0)) return Marshal.GetLastWin32Error();
            // FILE_PERSISTENT_ACLS; ERROR_NOT_SUPPORTED is fail-closed, never a fallback.
            return (flags & 0x00000008) != 0 ? 0 : 50;
        }
    }
    private static bool IsPrivate(string path, string sid) {
        if ((File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0) return false;
        var acl = Directory.GetAccessControl(path);
        var rules = acl.GetAccessRules(true, true, typeof(SecurityIdentifier));
        if (!acl.AreAccessRulesProtected || rules.Count != 1 ||
            acl.GetOwner(typeof(SecurityIdentifier)).Value != sid) return false;
        var rule = (FileSystemAccessRule)rules[0];
        return rule.IdentityReference.Value == sid && !rule.IsInherited &&
            rule.AccessControlType == AccessControlType.Allow &&
            rule.FileSystemRights == FileSystemRights.FullControl &&
            rule.InheritanceFlags == (InheritanceFlags.ContainerInherit |
                InheritanceFlags.ObjectInherit) && rule.PropagationFlags == PropagationFlags.None;
    }
    public static int Create(string path, string sid) {
        path = Path.GetFullPath(path);
        int volumeError = CheckVolume(Path.GetDirectoryName(path));
        if (volumeError != 0) return volumeError;
        IntPtr descriptor;
        uint size;
        string sddl = "O:" + sid + "D:P(A;OICI;FA;;;" + sid + ")";
        if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl, 1, out descriptor, out size)) return Marshal.GetLastWin32Error();
        try {
            var attributes = new Attributes {
                Length = Marshal.SizeOf(typeof(Attributes)), Descriptor = descriptor, Inherit = 0
            };
            if (!CreateDirectoryW(path, ref attributes)) return Marshal.GetLastWin32Error();
            volumeError = CheckVolume(path);
            if (volumeError != 0) return volumeError;
            // Verify before exposing the path; never repair an initially public directory.
            return IsPrivate(path, sid) ? 0 : 5;
        } finally { LocalFree(descriptor); }
    }
}
'@
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try {
    $path = [IO.Path]::GetFullPath($env:RUST_GATE_PRIVATE_DIRECTORY)
    $result = [PrivateDirectory]::Create($path, $identity.User.Value)
    if ($result -ne 0) { [Console]::Error.WriteLine("Private directory OS error: $result") }
    exit $result
} finally { $identity.Dispose() }
"#;

/// A fresh directory nobody else can read, the way `umask 077; mktemp -d`
/// made it.
pub(crate) fn private_directory(parent: &str, prefix: &str) -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    private_directory_at(parent, prefix, nanos)
}

/// Retry collisions from a fixed nonce; other filesystem failures stop immediately.
fn private_directory_at(parent: &str, prefix: &str, nanos: u128) -> Result<PathBuf, String> {
    let mut last = None;
    for attempt in 0u32..100 {
        let path = PathBuf::from(parent).join(format!(
            "{prefix}.{}{}",
            process::id(),
            u64::try_from(nanos)
                .unwrap_or(u64::MAX)
                .wrapping_add(u64::from(attempt))
        ));
        match create_private(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() != ErrorKind::AlreadyExists => {
                return Err(format!("cannot create {}: {error}", path.display()));
            }
            Err(error) => last = Some(error),
        }
    }
    let error = last.map_or_else(
        || "no attempt was made".to_owned(),
        |error| error.to_string(),
    );
    Err(format!(
        "cannot create a private directory under {parent}: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{create_private, private_directory, private_directory_at};
    use std::env;
    use std::fs;
    use std::io::ErrorKind;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_dir;
    use std::process;

    #[test]
    #[cfg(windows)]
    fn windows_creation_failures_preserve_both_collision_statuses() {
        use super::private_creation_error;
        use crate::runner::Failure;
        for code in [80, 183] {
            assert_eq!(
                private_creation_error(Failure {
                    code,
                    message: None
                })
                .kind(),
                ErrorKind::AlreadyExists
            );
        }
        let failure = private_creation_error(Failure {
            code: 5,
            message: None,
        });
        assert_eq!(failure.kind(), ErrorKind::Other);
        assert_eq!(
            failure.to_string(),
            "Windows private directory creation exited 5"
        );
        assert_eq!(
            private_creation_error(Failure {
                code: 3,
                message: Some("denied".into())
            })
            .to_string(),
            "denied"
        );
    }

    #[test]
    fn a_collision_retries_without_reusing_the_existing_directory() {
        let parent = env::temp_dir().join(format!("private-retry-{}", process::id()));
        fs::create_dir(&parent).unwrap();
        let occupied = parent.join(format!("gate.{}42", process::id()));
        create_private(&occupied).unwrap();
        fs::write(occupied.join("sentinel"), "untouched").unwrap();
        let created = private_directory_at(&parent.display().to_string(), "gate", 42).unwrap();
        assert_eq!(created, parent.join(format!("gate.{}43", process::id())));
        assert_eq!(
            fs::read_to_string(occupied.join("sentinel")).unwrap(),
            "untouched"
        );
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_private_directory_is_fresh_and_readable_by_its_owner_only() {
        let parent = env::temp_dir().join(format!("private-directories-{}", process::id()));
        fs::create_dir_all(&parent).unwrap();
        let first = private_directory(&parent.display().to_string(), "gate").unwrap();
        let second = private_directory(&parent.display().to_string(), "gate").unwrap();
        assert_ne!(first, second);
        #[cfg(unix)]
        {
            let mode = fs::metadata(&first).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        #[cfg(windows)]
        {
            fs::write(first.join("child"), "private").unwrap();
            let output = process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    r"
$ErrorActionPreference = 'Stop'
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
foreach ($path in @($env:ACL_PATH, (Join-Path $env:ACL_PATH 'child'))) {
    $acl = Get-Acl -LiteralPath $path
    $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    if ($rules.Count -ne 1 -or $rules[0].IdentityReference -ne $sid -or
        $rules[0].AccessControlType -ne 'Allow' -or
        $rules[0].FileSystemRights -ne 'FullControl') { exit 1 }
}
if (!(Get-Acl -LiteralPath $env:ACL_PATH).AreAccessRulesProtected) { exit 2 }
",
                ])
                .env("ACL_PATH", &first)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }
        fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn an_existing_destination_is_never_reused() {
        let root = env::temp_dir().join(format!("private-collision-{}", process::id()));
        fs::create_dir(&root).unwrap();
        assert_eq!(
            create_private(&root).unwrap_err().kind(),
            ErrorKind::AlreadyExists
        );
        fs::write(root.join("file"), "untouched").unwrap();
        assert_eq!(
            create_private(&root.join("file")).unwrap_err().kind(),
            ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(root.join("file")).unwrap(), "untouched");
        #[cfg(unix)]
        symlink(&root, root.join("link")).unwrap();
        #[cfg(windows)]
        symlink_dir(&root, root.join("link")).unwrap();
        assert_eq!(
            create_private(&root.join("link")).unwrap_err().kind(),
            ErrorKind::AlreadyExists
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_parent_that_does_not_exist_is_refused() {
        let parent = env::temp_dir().join(format!("missing-private-parent-{}", process::id()));
        let error = private_directory(&parent.display().to_string(), "gate").unwrap_err();
        #[cfg(windows)]
        assert!(
            error.ends_with("Windows private directory creation exited 3"),
            "{error}"
        );
        assert!(
            error.starts_with(&format!("cannot create {}", parent.join("gate.").display())),
            "{error}"
        );
    }
}
