use crate::GithubReleaseDetails;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use zed_extension_api::{self as zed, Result};

pub(crate) struct VerifiedBinary {
    path: String,
    directory: PathBuf,
    digest: [u8; 32],
}

pub(crate) trait ReleaseHost {
    fn root(&self) -> PathBuf;
    fn platform(&self) -> (zed::Os, zed::Architecture);
    fn latest_release(&mut self) -> Result<zed::GithubRelease>;
    fn download(&mut self, url: &str, directory: &str, kind: zed::DownloadedFileType)
        -> Result<()>;
    fn sidecar(&mut self, url: &str) -> Result<Vec<u8>>;
}

pub(crate) struct ZedReleaseHost<'a>(pub &'a zed::LanguageServerId);

impl ReleaseHost for ZedReleaseHost<'_> {
    fn root(&self) -> PathBuf {
        PathBuf::from(".")
    }
    fn platform(&self) -> (zed::Os, zed::Architecture) {
        zed::current_platform()
    }
    fn latest_release(&mut self) -> Result<zed::GithubRelease> {
        zed::set_language_server_installation_status(
            self.0,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        zed::latest_github_release(
            "sims1253/ry",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )
    }
    fn download(
        &mut self,
        url: &str,
        directory: &str,
        kind: zed::DownloadedFileType,
    ) -> Result<()> {
        zed::set_language_server_installation_status(
            self.0,
            &zed::LanguageServerInstallationStatus::Downloading,
        );
        zed::download_file(url, directory, kind)
    }
    fn sidecar(&mut self, url: &str) -> Result<Vec<u8>> {
        use zed::http_client::{HttpMethod, HttpRequest, RedirectPolicy};
        let response = HttpRequest::builder()
            .method(HttpMethod::Get)
            .url(url)
            .redirect_policy(RedirectPolicy::FollowAll)
            .build()?
            .fetch()?;
        Ok(response.body)
    }
}

pub(crate) fn verified_binary(
    host: &mut impl ReleaseHost,
    cache: &mut Option<VerifiedBinary>,
) -> Result<String> {
    if let Some(binary) = cache.take() {
        // Rehash on every command, including restarts. A process-local digest
        // came from HTTPS; an adjacent on-disk digest is not a trust anchor.
        let exists = fs::symlink_metadata(&binary.path)
            .map(|_| true)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(false)
                } else {
                    Err(error)
                }
            })
            .map_err(|error| remove_failed_install(&binary.directory, error.to_string()))?;
        if exists {
            if let Err(error) = verify(&binary.path, &binary.digest) {
                return Err(remove_failed_install(&binary.directory, error));
            }
            let path = binary.path.clone();
            *cache = Some(binary);
            return Ok(path);
        }
    }
    let release = host.latest_release()?;
    // Release tags become directories. Refuse separators and traversal before
    // constructing a path that either installation or cleanup could touch.
    if release.version.is_empty()
        || !release
            .version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c))
    {
        return Err("Invalid release version in download metadata".into());
    }
    let (platform, arch) = host.platform();
    let details = GithubReleaseDetails::new(platform, arch, release.version);
    let directory = host.root().join(&details.downloaded_directory);
    let path = host.root().join(&details.downloaded_binary_path);
    let result = (|| {
        let binary_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid binary filename")?;
        let stem = details
            .asset_name
            .strip_suffix(".tar.gz")
            .or_else(|| details.asset_name.strip_suffix(".zip"))
            .ok_or("Invalid archive name")?;
        let sidecar_name = format!("{stem}.bin.sha256");
        let asset = |name: &str| {
            release.assets.iter().find(|asset| asset.name == name)
            .ok_or_else(|| format!("Release is missing {name}; automatic downloads require executable checksums (ry 0.9.0 or newer)"))
        };
        let digest = parse_sidecar(
            &host.sidecar(&asset(&sidecar_name)?.download_url)?,
            binary_name,
        )?;
        let archive = asset(&details.asset_name)?;
        if !path.try_exists().map_err(|e| e.to_string())? {
            host.download(
                &archive.download_url,
                directory.to_str().ok_or("Invalid download directory")?,
                details.downloaded_file_type,
            )?;
        }
        let path = path.to_str().ok_or("Invalid binary path")?.to_owned();
        verify(&path, &digest)?;
        Ok(VerifiedBinary {
            path,
            directory: directory.clone(),
            digest,
        })
    })();
    match result {
        Ok(binary) => {
            // Retire old downloads only after the replacement has passed
            // verification; leave unrelated extension state untouched.
            if let Ok(entries) = fs::read_dir(host.root()) {
                for entry in entries.flatten() {
                    if entry.path() != directory
                        && entry
                            .file_name()
                            .to_str()
                            .is_some_and(|name| name.starts_with("ry-"))
                        && entry.file_type().is_ok_and(|kind| kind.is_dir())
                    {
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
            let path = binary.path.clone();
            *cache = Some(binary);
            Ok(path)
        }
        Err(error) => Err(remove_failed_install(&directory, error)),
    }
}

fn remove_failed_install(directory: &Path, error: String) -> String {
    match fs::remove_dir_all(directory) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup) => format!(
            "{error}; could not remove failed download {}: {cleanup}",
            directory.display()
        ),
    }
}

fn parse_sidecar(bytes: &[u8], binary_name: &str) -> Result<[u8; 32]> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Checksum is not UTF-8")?;
    let line = text.strip_suffix('\n').unwrap_or(text);
    let (hash, filename) = line
        .split_once("  ")
        .ok_or("Malformed executable checksum")?;
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) || filename != binary_name {
        return Err("Malformed executable checksum or wrong binary filename".into());
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16)
            .map_err(|_| "Malformed executable checksum")?;
    }
    Ok(digest)
}

fn verify(path: &str, expected: &[u8; 32]) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("Cannot inspect downloaded binary: {e}"))?;
    if !metadata.file_type().is_file() {
        return Err("Downloaded binary is not a regular file".into());
    }
    let mut file =
        fs::File::open(path).map_err(|e| format!("Cannot read downloaded binary: {e}"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("Cannot hash downloaded binary: {e}"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if hash.finalize().as_slice() != expected {
        return Err(
            "Downloaded ry executable does not match its published SHA-256 checksum".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RyExtension;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeHost {
        root: PathBuf,
        platform: zed::Os,
        version: String,
        sidecar: Result<Vec<u8>>,
        missing_sidecar: bool,
        download_fails: bool,
        downloads: usize,
        lookups: usize,
        sidecar_reads: usize,
    }
    impl FakeHost {
        fn new(platform: zed::Os) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "ry-zed-verify-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let filename = if platform == zed::Os::Windows {
                "ry.exe"
            } else {
                "ry"
            };
            Self {
                root,
                platform,
                version: "v0.9.0".into(),
                sidecar: Ok(
                    format!("{:x}  {filename}\n", Sha256::digest(b"server bytes")).into_bytes(),
                ),
                missing_sidecar: false,
                download_fails: false,
                downloads: 0,
                lookups: 0,
                sidecar_reads: 0,
            }
        }
        fn details(&self) -> GithubReleaseDetails {
            GithubReleaseDetails::new(
                self.platform,
                zed::Architecture::X8664,
                self.version.clone(),
            )
        }
        fn binary(&self) -> PathBuf {
            self.root.join(self.details().downloaded_binary_path)
        }
        fn directory(&self) -> PathBuf {
            self.root.join(self.details().downloaded_directory)
        }
        fn seed(&self, bytes: &[u8]) {
            fs::create_dir_all(self.binary().parent().unwrap()).unwrap();
            fs::write(self.binary(), bytes).unwrap();
        }
    }
    impl Drop for FakeHost {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    impl ReleaseHost for FakeHost {
        fn root(&self) -> PathBuf {
            self.root.clone()
        }
        fn platform(&self) -> (zed::Os, zed::Architecture) {
            (self.platform, zed::Architecture::X8664)
        }
        fn latest_release(&mut self) -> Result<zed::GithubRelease> {
            self.lookups += 1;
            let details = self.details();
            let stem = details
                .asset_name
                .strip_suffix(".tar.gz")
                .or_else(|| details.asset_name.strip_suffix(".zip"))
                .unwrap();
            let mut assets = vec![zed::GithubReleaseAsset {
                name: details.asset_name.clone(),
                download_url: "archive".into(),
            }];
            if !self.missing_sidecar {
                assets.push(zed::GithubReleaseAsset {
                    name: format!("{stem}.bin.sha256"),
                    download_url: "checksum".into(),
                });
            }
            Ok(zed::GithubRelease {
                version: self.version.clone(),
                assets,
            })
        }
        fn download(
            &mut self,
            url: &str,
            directory: &str,
            kind: zed::DownloadedFileType,
        ) -> Result<()> {
            assert_eq!(url, "archive");
            assert_eq!(Path::new(directory), self.directory());
            assert_eq!(kind, self.details().downloaded_file_type);
            self.downloads += 1;
            self.seed(b"server bytes");
            if self.download_fails {
                Err("partial download".into())
            } else {
                Ok(())
            }
        }
        fn sidecar(&mut self, url: &str) -> Result<Vec<u8>> {
            assert_eq!(url, "checksum");
            self.sidecar_reads += 1;
            self.sidecar.clone()
        }
    }
    fn extension() -> RyExtension {
        RyExtension {
            cached_binary: None,
        }
    }
    fn command(extension: &mut RyExtension, host: &mut FakeHost) -> Result<zed::Command> {
        extension.command_with_host(host, None, None, None)
    }

    #[test]
    fn verified_download_returns_command_for_each_archive_layout() {
        for platform in [zed::Os::Linux, zed::Os::Mac, zed::Os::Windows] {
            let mut host = FakeHost::new(platform);
            let mut extension = extension();
            let first = command(&mut extension, &mut host).unwrap();
            assert_eq!(Path::new(&first.command), host.binary());
            assert_eq!(first.args, ["server"]);
            assert_eq!(
                command(&mut extension, &mut host).unwrap().command,
                first.command
            );
            assert_eq!(
                (host.downloads, host.lookups, host.sidecar_reads),
                (1, 1, 1)
            );
        }
    }
    #[test]
    fn missing_malformed_mismatched_and_unavailable_sidecars_fail_closed() {
        for case in [
            "missing",
            "malformed",
            "mismatch",
            "wrong filename",
            "http error",
            "extra line",
        ] {
            let mut host = FakeHost::new(zed::Os::Linux);
            // A prior install must never bypass verification, including a
            // version downloaded by an older extension without a checksum.
            host.seed(b"server bytes");
            match case {
                "missing" => host.missing_sidecar = true,
                "malformed" => host.sidecar = Ok(b"not a digest".to_vec()),
                "mismatch" => host.sidecar = Ok(format!("{}  ry\n", "0".repeat(64)).into_bytes()),
                "wrong filename" => {
                    host.sidecar =
                        Ok(format!("{:x}  ry.exe\n", Sha256::digest(b"server bytes")).into_bytes())
                }
                "http error" => host.sidecar = Err("HTTP 404".into()),
                "extra line" => host.sidecar.as_mut().unwrap().extend_from_slice(b"extra\n"),
                _ => unreachable!(),
            }
            let mut extension = extension();
            assert!(command(&mut extension, &mut host).is_err(), "{case}");
            assert!(!host.directory().exists(), "{case}");
            assert!(extension.cached_binary.is_none());
            assert_eq!(host.downloads, 0);
        }
    }
    #[test]
    fn fresh_mismatch_and_partial_download_remove_extracted_files() {
        for partial in [false, true] {
            let mut host = FakeHost::new(zed::Os::Linux);
            host.download_fails = partial;
            if !partial {
                host.sidecar = Ok(format!("{}  ry\n", "0".repeat(64)).into_bytes());
            }
            assert!(command(&mut extension(), &mut host).is_err());
            assert_eq!(host.downloads, 1);
            assert!(!host.directory().exists());
        }
    }
    #[test]
    fn existing_disk_binary_is_verified_and_rechecked_on_restart() {
        let mut host = FakeHost::new(zed::Os::Linux);
        host.seed(b"server bytes");
        let mut extension = extension();
        command(&mut extension, &mut host).unwrap();
        assert_eq!(host.downloads, 0);
        host.seed(b"modified after verification");
        assert!(command(&mut extension, &mut host).is_err());
        assert!(!host.directory().exists());
        assert!(extension.cached_binary.is_none());
    }
    #[test]
    fn removed_cache_is_downloaded_and_verified_again() {
        let mut host = FakeHost::new(zed::Os::Linux);
        let mut extension = extension();
        command(&mut extension, &mut host).unwrap();
        fs::remove_dir_all(host.directory()).unwrap();
        command(&mut extension, &mut host).unwrap();
        assert_eq!(
            (host.downloads, host.lookups, host.sidecar_reads),
            (2, 2, 2)
        );
    }
    #[test]
    fn settings_and_path_binaries_keep_precedence_and_arguments() {
        for (settings, path, expected) in [
            (Some("custom"), Some("path"), "custom"),
            (None, Some("path"), "path"),
        ] {
            let mut host = FakeHost::new(zed::Os::Linux);
            let result = extension()
                .command_with_host(
                    &mut host,
                    settings.map(str::to_owned),
                    path.map(str::to_owned),
                    Some(vec!["server".into(), "--stdio".into()]),
                )
                .unwrap();
            assert_eq!(result.command, expected);
            assert_eq!(result.args, ["server", "--stdio"]);
            assert_eq!(host.lookups, 0);
        }
    }
    #[test]
    fn legacy_release_without_sidecar_and_unsafe_version_are_refused() {
        let mut host = FakeHost::new(zed::Os::Linux);
        host.version = "v0.8.0".into();
        host.missing_sidecar = true;
        host.seed(b"unverified legacy binary");
        assert!(command(&mut extension(), &mut host).is_err());
        assert!(!host.directory().exists());
        host.version = "../../outside".into();
        assert!(command(&mut extension(), &mut host)
            .unwrap_err()
            .contains("Invalid release version"));
        assert_eq!(host.downloads, 0);
    }
    #[test]
    fn failed_install_preserves_previous_version_and_success_retires_it() {
        let mut host = FakeHost::new(zed::Os::Linux);
        let previous = host.root.join("ry-v0.8.0");
        let unrelated = host.root.join("settings");
        fs::create_dir(&previous).unwrap();
        fs::create_dir(&unrelated).unwrap();
        host.missing_sidecar = true;
        assert!(command(&mut extension(), &mut host).is_err());
        assert!(previous.exists());
        host.missing_sidecar = false;
        command(&mut extension(), &mut host).unwrap();
        assert!(!previous.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn directory_in_place_of_cached_binary_is_refused_and_removed() {
        let mut host = FakeHost::new(zed::Os::Linux);
        let mut extension = extension();
        command(&mut extension, &mut host).unwrap();
        fs::remove_file(host.binary()).unwrap();
        fs::create_dir(host.binary()).unwrap();
        assert!(command(&mut extension, &mut host).is_err());
        assert!(!host.directory().exists());
    }
}
