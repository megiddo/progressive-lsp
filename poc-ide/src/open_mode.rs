//! How the POC opens a workspace: native serve vs one Linux container host.

use std::path::PathBuf;

/// OS class of the **editor** process. The intelligence host is Linux.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    Linux,
    Other,
}

impl HostOs {
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    pub fn is_linux(self) -> bool {
        matches!(self, Self::Linux)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Other => "other",
        }
    }

    /// Container open is a Darwin/Windows stand-in for Graviton. Linux opens natively.
    pub fn shows_container_open(self) -> bool {
        !self.is_linux()
    }
}

/// One intelligence process per workspace. Not a split T1/T3 LSP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    /// Local `progressive-lsp serve`. T3 only when [`HostOs::Linux`].
    Native,
    /// One Linux container is the serve host (T1/T2/T3). Client stays native.
    Container,
}

impl OpenMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Container => "container",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "native" => Some(Self::Native),
            "container" => Some(Self::Container),
            _ => None,
        }
    }

    /// T3 packs run on the Linux serve host. Fast native open on a laptop does not.
    pub fn offers_t3(self, host: HostOs) -> bool {
        match self {
            Self::Native => host.is_linux(),
            Self::Container => true,
        }
    }

    pub fn folder_label(self) -> &'static str {
        match self {
            Self::Native => "Open Folder…",
            Self::Container => "Open Folder in Container…",
        }
    }

    /// Linux is already the intelligence host. `--container` is a no-op there.
    pub fn for_host(self, host: HostOs) -> Self {
        if host.is_linux() {
            Self::Native
        } else {
            self
        }
    }
}

/// Whether this open may paint T3 as a real engine path (not “use container”).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T3HostOffer {
    Offered,
    NeedsContainer,
}

impl T3HostOffer {
    pub fn from_open(host: HostOs, mode: OpenMode) -> Self {
        if mode.offers_t3(host) {
            Self::Offered
        } else {
            Self::NeedsContainer
        }
    }

    pub fn is_offered(self) -> bool {
        matches!(self, Self::Offered)
    }
}

/// CLI flags for the composition root. Tests parse strings; they do not spawn eframe.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchFlags {
    pub folder: Option<PathBuf>,
    pub file: Option<PathBuf>,
    pub control_socket: Option<PathBuf>,
    pub container: bool,
}

impl LaunchFlags {
    pub fn open_mode(&self) -> OpenMode {
        if self.container {
            OpenMode::Container
        } else {
            OpenMode::Native
        }
    }
}

/// Parse poc-ide argv (after the binary name).
pub fn parse_launch_args(args: impl Iterator<Item = String>) -> LaunchFlags {
    let mut flags = LaunchFlags::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--folder=") {
            flags.folder = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--file=") {
            flags.file = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--control-socket=") {
            flags.control_socket = Some(PathBuf::from(value));
        } else if arg == "--folder" {
            flags.folder = args.next().map(PathBuf::from);
        } else if arg == "--file" {
            flags.file = args.next().map(PathBuf::from);
        } else if arg == "--container" {
            flags.container = true;
        } else if arg == "--control-socket" {
            match args.peek() {
                Some(next) if !next.starts_with('-') => {
                    flags.control_socket = args.next().map(PathBuf::from);
                }
                _ => {
                    flags.control_socket = Some(std::env::temp_dir().join("poc-ide-control.sock"));
                }
            }
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn host_os_and_open_mode_t3_offer() {
        assert_eq!(HostOs::Linux.as_str(), "linux");
        assert_eq!(HostOs::Other.as_str(), "other");
        assert!(HostOs::Linux.is_linux());
        assert!(!HostOs::Other.is_linux());
        assert!(!HostOs::Linux.shows_container_open());
        assert!(HostOs::Other.shows_container_open());
        assert_eq!(HostOs::current().is_linux(), cfg!(target_os = "linux"));

        assert_eq!(OpenMode::Native.as_str(), "native");
        assert_eq!(OpenMode::Container.as_str(), "container");
        assert_eq!(OpenMode::parse("native"), Some(OpenMode::Native));
        assert_eq!(OpenMode::parse("container"), Some(OpenMode::Container));
        assert_eq!(OpenMode::parse("docker"), None);
        assert_eq!(OpenMode::Native.folder_label(), "Open Folder…");
        assert_eq!(
            OpenMode::Container.folder_label(),
            "Open Folder in Container…"
        );

        assert!(OpenMode::Native.offers_t3(HostOs::Linux));
        assert!(!OpenMode::Native.offers_t3(HostOs::Other));
        assert!(OpenMode::Container.offers_t3(HostOs::Linux));
        assert!(OpenMode::Container.offers_t3(HostOs::Other));
        assert_eq!(
            OpenMode::Container.for_host(HostOs::Linux),
            OpenMode::Native
        );
        assert_eq!(
            OpenMode::Container.for_host(HostOs::Other),
            OpenMode::Container
        );
        assert_eq!(OpenMode::Native.for_host(HostOs::Other), OpenMode::Native);

        assert_eq!(
            T3HostOffer::from_open(HostOs::Other, OpenMode::Native),
            T3HostOffer::NeedsContainer
        );
        assert_eq!(
            T3HostOffer::from_open(HostOs::Linux, OpenMode::Native),
            T3HostOffer::Offered
        );
        assert_eq!(
            T3HostOffer::from_open(HostOs::Other, OpenMode::Container),
            T3HostOffer::Offered
        );
        assert!(T3HostOffer::Offered.is_offered());
        assert!(!T3HostOffer::NeedsContainer.is_offered());
        assert_ne!(OpenMode::Native, OpenMode::Container);
        assert_ne!(HostOs::Linux, HostOs::Other);
    }

    #[test]
    fn parse_launch_args_folder_file_container_and_socket() {
        let empty = parse_launch_args(std::iter::empty());
        assert_eq!(empty, LaunchFlags::default());
        assert_eq!(empty.open_mode(), OpenMode::Native);

        let flags = parse_launch_args(
            [
                "--folder".into(),
                "/ws".into(),
                "--container".into(),
                "--file=/ws/a.rs".into(),
            ]
            .into_iter(),
        );
        assert_eq!(flags.folder.as_deref(), Some(Path::new("/ws")));
        assert_eq!(flags.file.as_deref(), Some(Path::new("/ws/a.rs")));
        assert!(flags.container);
        assert_eq!(flags.open_mode(), OpenMode::Container);

        let eq = parse_launch_args(["--folder=/proj".into()].into_iter());
        assert_eq!(eq.folder.as_deref(), Some(Path::new("/proj")));
        assert!(!eq.container);

        let sock = parse_launch_args(["--control-socket=/tmp/x.sock".into()].into_iter());
        assert_eq!(
            sock.control_socket.as_deref(),
            Some(Path::new("/tmp/x.sock"))
        );
        let split =
            parse_launch_args(["--control-socket".into(), "/tmp/y.sock".into()].into_iter());
        assert_eq!(
            split.control_socket.as_deref(),
            Some(Path::new("/tmp/y.sock"))
        );
        let bare = parse_launch_args(["--control-socket".into()].into_iter());
        assert!(bare
            .control_socket
            .as_ref()
            .is_some_and(|p| p.ends_with("poc-ide-control.sock")));
    }
}
