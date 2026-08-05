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

        // 1. Use user-specified path to the `ry` binary, if specified.
        if let Some(path) = binary_settings.and_then(|binary_settings| binary_settings.path) {
            return Ok(RyBinary {
                path,
                args: binary_args,
            });
        }

        // 2. Use binary on the `PATH`, if it exists.
        if let Some(path) = worktree.which("ry") {
            return Ok(RyBinary {
                path,
                args: binary_args,
            });
        }

        // 3. Use binary from a previous download, if we can find one.
        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|stat| stat.is_file()) {
                return Ok(RyBinary {
                    path: path.clone(),
                    args: binary_args,
                });
            }
        }

        // 4. All other methods failed; download the binary from the latest
        //    GitHub release.
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
        let settings = LspSettings::for_worktree(server_id.as_ref(), worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.settings.clone())
            .unwrap_or_default();
        Ok(Some(settings))
    }
}

zed::register_extension!(RyExtension);

#[cfg(test)]
mod test {
    use crate::GithubReleaseDetails;

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
