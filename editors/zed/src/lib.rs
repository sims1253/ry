use std::fs;
use zed::LanguageServerId;
use zed_extension_api::{self as zed, settings::LspSettings, Result};

struct RyBinary {
    path: String,
    args: Option<Vec<String>>,
}

struct RyExtension {
    cached_binary_path: Option<String>,
}

/// Describes the structure of a GitHub release asset for a given platform and
/// architecture.
///
/// This exists so that path construction can be unit-tested without network
/// access. Getting it wrong is a runtime download failure that no compile
/// step catches.
#[derive(Debug, PartialEq)]
struct GithubReleaseDetails {
    /// The name of the GitHub asset that contains the binary.
    asset_name: String,

    /// The type of file the asset is compressed as.
    downloaded_file_type: zed::DownloadedFileType,

    /// The on-disk directory the asset is extracted into, relative to the
    /// extension's working directory.
    downloaded_directory: String,

    /// The on-disk path to the binary, nested within
    /// `downloaded_directory`, relative to the extension's working directory.
    downloaded_binary_path: String,
}

impl RyExtension {
    fn language_server_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<RyBinary> {
        // Pull `BinarySettings`, if they exist. This includes user-specified
        // path to the binary and any user-specified arguments for the binary.
        let binary_settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.binary);

        // Pass through user-specified binary arguments no matter what method
        // is used to get the binary. If no arguments are supplied we fall back
        // to just `server` as the sole argument.
        let binary_args = binary_settings
            .as_ref()
            .and_then(|binary_settings| binary_settings.arguments.clone());

        // The user-specified path, a `ry` on the PATH, or a previous
        // download, in that order. A cached download is only a candidate
        // while its file is still on disk: Zed may clear the extension's
        // working directory between versions, leaving
        // `cached_binary_path` dangling.
        let cached_path = self
            .cached_binary_path
            .as_deref()
            .filter(|path| Self::cached_binary_on_disk(path));

        if let Some(path) = Self::resolve_binary_path(
            binary_settings.and_then(|binary_settings| binary_settings.path),
            worktree.which("ry"),
            cached_path,
        ) {
            return Ok(RyBinary {
                path,
                args: binary_args,
            });
        }

        // All local candidates failed; download the binary from the latest
        // GitHub release.
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = zed::latest_github_release(
            "sims1253/ry",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (platform, arch) = zed::current_platform();
        let release_details = GithubReleaseDetails::new(platform, arch, release.version);

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == release_details.asset_name)
            .ok_or_else(|| {
                format!(
                    "No asset found matching {asset_name:?}",
                    asset_name = release_details.asset_name
                )
            })?;

        if !fs::metadata(&release_details.downloaded_binary_path).is_ok_and(|stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &release_details.downloaded_directory,
                release_details.downloaded_file_type,
            )
            .map_err(|error| format!("Failed to download file: {error}"))?;

            // NOTE: downloaded binaries are NOT integrity-verified yet.
            // Issue #80 tracks that work.
            //
            // Releases publish `.sha256` sidecars for the *archive*
            // (`ry-cli-<target>.tar.gz.sha256`), but this code path
            // only ever holds the *extracted* executable:
            // `download_file` extracts in the same call that fetches.
            // Checking the archive digest here would compare two
            // different artifacts. The fix is to publish a digest of
            // the executable itself and check that.

            // Clean out other entries in our personal extension directory;
            // this may include outdated versions of the extension, so it is
            // good hygiene.
            let entries = fs::read_dir(".")
                .map_err(|error| format!("Failed to list working directory: {error}"))?;

            for entry in entries {
                let entry =
                    entry.map_err(|error| format!("Failed to load directory entry: {error}"))?;
                if entry.file_name().to_str() != Some(&release_details.downloaded_directory) {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }

        // Update cache path for later.
        self.cached_binary_path = Some(release_details.downloaded_binary_path.clone());

        Ok(RyBinary {
            path: release_details.downloaded_binary_path,
            args: binary_args,
        })
    }
}

impl GithubReleaseDetails {
    fn new(
        platform: zed_extension_api::Os,
        arch: zed_extension_api::Architecture,
        version: String,
    ) -> Self {
        // Note the asymmetry: the asset prefix is `ry-cli` (cargo-dist uses
        // the package name) while the binary inside is `ry`.
        let asset_stem = format!(
            "ry-cli-{arch}-{os}",
            arch = match arch {
                zed::Architecture::Aarch64 => "aarch64",
                zed::Architecture::X86 => "x86",
                zed::Architecture::X8664 => "x86_64",
            },
            os = match platform {
                zed::Os::Mac => "apple-darwin",
                zed::Os::Linux => "unknown-linux-gnu",
                zed::Os::Windows => "pc-windows-msvc",
            }
        );

        let asset_name = format!(
            "{asset_stem}.{suffix}",
            suffix = match platform {
                zed::Os::Mac | zed::Os::Linux => "tar.gz",
                zed::Os::Windows => "zip",
            }
        );

        let downloaded_file_type = match platform {
            zed::Os::Mac | zed::Os::Linux => zed::DownloadedFileType::GzipTar,
            zed::Os::Windows => zed::DownloadedFileType::Zip,
        };

        let downloaded_directory = format!("ry-{version}");

        // unix:   binary is `{asset_stem}/ry` inside the tarball
        // windows: binary is `ry.exe` flat at the archive root
        let downloaded_binary_path = match platform {
            zed::Os::Mac | zed::Os::Linux => format!("{downloaded_directory}/{asset_stem}/ry"),
            zed::Os::Windows => format!("{downloaded_directory}/ry.exe"),
        };

        Self {
            asset_name,
            downloaded_file_type,
            downloaded_directory,
            downloaded_binary_path,
        }
    }
}

impl RyExtension {
    /// Prefer settings, then PATH, then an existing cached download.
    fn resolve_binary_path(
        settings_path: Option<String>,
        path_lookup: Option<String>,
        cached_path: Option<&str>,
    ) -> Option<String> {
        settings_path
            .or(path_lookup)
            .or_else(|| cached_path.map(str::to_owned))
    }

    /// Whether a cached download still exists as a regular file.
    fn cached_binary_on_disk(path: &str) -> bool {
        fs::metadata(path).is_ok_and(|stat| stat.is_file())
    }

    /// Map Zed settings into the server settings envelope.
    /// Rejects malformed values with actionable errors.
    fn map_settings(lsp_settings: &LspSettings) -> Result<zed_extension_api::serde_json::Value> {
        let settings = lsp_settings.settings.clone().unwrap_or_default();

        // Validate known settings fields if present.
        if let Some(obj) = settings.as_object() {
            if let Some(min_confidence) = obj.get("minConfidence") {
                if let Some(s) = min_confidence.as_str() {
                    if !matches!(s, "low" | "medium" | "high") {
                        return Err(format!(
                            "Invalid minConfidence '{s}'. Must be 'low', 'medium', or 'high'."
                        ));
                    }
                }
            }
        }

        Ok(settings)
    }
}

impl zed::Extension for RyExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let ry_binary = self.language_server_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            command: ry_binary.path,
            args: ry_binary.args.unwrap_or_else(|| vec!["server".into()]),
            env: vec![],
        })
    }

    fn language_server_initialization_options(
        &mut self,
        server_id: &LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.initialization_options.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }

    fn language_server_workspace_configuration(
        &mut self,
        server_id: &LanguageServerId,
        worktree: &zed_extension_api::Worktree,
    ) -> Result<Option<zed_extension_api::serde_json::Value>> {
        let lsp_settings = LspSettings::for_worktree(server_id.as_ref(), worktree).ok();
        match lsp_settings {
            Some(ls) => {
                let settings = Self::map_settings(&ls)?;
                Ok(Some(settings))
            }
            None => Ok(Some(zed_extension_api::serde_json::Value::Object(
                Default::default(),
            ))),
        }
    }
}

zed::register_extension!(RyExtension);

#[cfg(test)]
mod test {
    use crate::{GithubReleaseDetails, RyExtension};

    #[test]
    fn binary_path_precedence() {
        for (settings, path, cache, expected) in [
            (
                Some("settings"),
                Some("path"),
                Some("cache"),
                Some("settings"),
            ),
            (None, Some("path"), Some("cache"), Some("path")),
            (None, None, Some("cache"), Some("cache")),
            (None, None, None, None),
        ] {
            assert_eq!(
                RyExtension::resolve_binary_path(
                    settings.map(str::to_owned),
                    path.map(str::to_owned),
                    cache
                )
                .as_deref(),
                expected,
            );
        }
    }

    /// The cache candidate is offered only while the downloaded file is
    /// still on disk, so a cache wiped between versions falls through to
    /// a download rather than returning a dangling path. Only a regular
    /// file counts: a directory at the cached path is not a usable
    /// binary and must fall through to a download as well.
    #[test]
    fn cached_binary_on_disk_requires_a_file() {
        let directory = std::env::temp_dir();
        assert!(!RyExtension::cached_binary_on_disk(
            directory.to_str().unwrap()
        ));

        let file = std::env::temp_dir().join(format!("ry-zed-cache-{}", std::process::id()));
        std::fs::write(&file, b"").unwrap();
        let path = file.to_str().unwrap();
        assert!(RyExtension::cached_binary_on_disk(path));
        std::fs::remove_file(&file).unwrap();
        assert!(!RyExtension::cached_binary_on_disk(path));
        assert!(!RyExtension::cached_binary_on_disk(
            "ry-no-such-cached-binary"
        ));
    }

    /// Tests path construction for all six cargo-dist targets, locking down
    /// the asset prefix / binary name asymmetry: the asset is
    /// `ry-cli-<target>` but the binary inside is `ry`.
    #[test]
    fn test_github_release_details() {
        // --- macOS (aarch64) ---
        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Mac,
                zed_extension_api::Architecture::Aarch64,
                String::from("0.8.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("ry-cli-aarch64-apple-darwin.tar.gz"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::GzipTar,
                downloaded_directory: String::from("ry-0.8.0"),
                downloaded_binary_path: String::from("ry-0.8.0/ry-cli-aarch64-apple-darwin/ry")
            }
        );

        // --- macOS (x86_64) ---
        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Mac,
                zed_extension_api::Architecture::X8664,
                String::from("0.8.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("ry-cli-x86_64-apple-darwin.tar.gz"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::GzipTar,
                downloaded_directory: String::from("ry-0.8.0"),
                downloaded_binary_path: String::from("ry-0.8.0/ry-cli-x86_64-apple-darwin/ry")
            }
        );

        // --- Linux (aarch64) ---
        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Linux,
                zed_extension_api::Architecture::Aarch64,
                String::from("0.8.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("ry-cli-aarch64-unknown-linux-gnu.tar.gz"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::GzipTar,
                downloaded_directory: String::from("ry-0.8.0"),
                downloaded_binary_path: String::from(
                    "ry-0.8.0/ry-cli-aarch64-unknown-linux-gnu/ry"
                )
            }
        );

        // --- Linux (x86_64) ---
        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Linux,
                zed_extension_api::Architecture::X8664,
                String::from("0.8.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("ry-cli-x86_64-unknown-linux-gnu.tar.gz"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::GzipTar,
                downloaded_directory: String::from("ry-0.8.0"),
                downloaded_binary_path: String::from("ry-0.8.0/ry-cli-x86_64-unknown-linux-gnu/ry")
            }
        );

        // --- Windows (aarch64) ---
        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Windows,
                zed_extension_api::Architecture::Aarch64,
                String::from("0.8.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("ry-cli-aarch64-pc-windows-msvc.zip"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::Zip,
                downloaded_directory: String::from("ry-0.8.0"),
                downloaded_binary_path: String::from("ry-0.8.0/ry.exe")
            }
        );

        // --- Windows (x86_64) ---
        assert_eq!(
            GithubReleaseDetails::new(
                zed_extension_api::Os::Windows,
                zed_extension_api::Architecture::X8664,
                String::from("0.8.0"),
            ),
            GithubReleaseDetails {
                asset_name: String::from("ry-cli-x86_64-pc-windows-msvc.zip"),
                downloaded_file_type: zed_extension_api::DownloadedFileType::Zip,
                downloaded_directory: String::from("ry-0.8.0"),
                downloaded_binary_path: String::from("ry-0.8.0/ry.exe")
            }
        );
    }
}
