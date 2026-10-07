//! The computer the launcher runs on, named like the site's download routes
//! (`macos-arm64`, `macos-x86_64`, `linux-x86_64`, `windows-x86_64`).

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Linux,
    Windows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    Arm64,
    X86_64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    /// This computer. On a Mac the launcher may itself run under Rosetta: the hardware decides.
    pub fn current() -> Self {
        let os = if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        };
        let arch = if cfg!(target_arch = "aarch64") { Arch::Arm64 } else { Arch::X86_64 };
        #[cfg(target_os = "macos")]
        let arch = if arch == Arch::X86_64
            && std::process::Command::new("sysctl").args(["-n", "hw.optional.arm64"]).output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
        {
            Arch::Arm64
        } else {
            arch
        };
        if let Ok(forced) = std::env::var("LSUITE_PLATFORM")
            && let Some(p) = Self::parse(&forced)
        {
            return p;
        }
        Platform { os, arch }
    }

    pub fn parse(key: &str) -> Option<Self> {
        let (os, arch) = key.trim().split_once('-')?;
        let os = match os {
            "macos" => Os::Macos,
            "linux" => Os::Linux,
            "windows" => Os::Windows,
            _ => return None,
        };
        let arch = match arch {
            "arm64" | "aarch64" => Arch::Arm64,
            "x86_64" | "x64" => Arch::X86_64,
            _ => return None,
        };
        Some(Platform { os, arch })
    }

    pub fn key(&self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Macos, Arch::Arm64) => "macos-arm64",
            (Os::Macos, Arch::X86_64) => "macos-x86_64",
            (Os::Linux, Arch::Arm64) => "linux-arm64",
            (Os::Linux, Arch::X86_64) => "linux-x86_64",
            (Os::Windows, Arch::Arm64) => "windows-arm64",
            (Os::Windows, Arch::X86_64) => "windows-x86_64",
        }
    }

    /// "macOS", "Linux", "Windows".
    pub fn os_name(&self) -> &'static str {
        match self.os {
            Os::Macos => "macOS",
            Os::Linux => "Linux",
            Os::Windows => "Windows",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for k in ["macos-arm64", "macos-x86_64", "linux-x86_64", "windows-x86_64"] {
            assert_eq!(Platform::parse(k).unwrap().key(), k);
        }
        assert_eq!(Platform::parse("macos-aarch64").unwrap().key(), "macos-arm64");
        assert!(Platform::parse("beos-x86").is_none());
    }
}
