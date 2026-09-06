use zed::LanguageServerId;
use zed_extension_api::{self as zed, settings::LspSettings, Result};

mod download;
use download::{ReleaseHost, VerifiedBinary, ZedReleaseHost};

struct RyExtension {
    cached_binary: Option<VerifiedBinary>,
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
    /// This is also the test boundary: settings/PATH come from the worktree,
    /// while release lookup and downloads come from the injected host.
    fn command_with_host(
        &mut self,
        host: &mut impl ReleaseHost,
        settings_path: Option<String>,
        path_lookup: Option<String>,
        args: Option<Vec<String>>,
    ) -> Result<zed::Command> {
        let path = if let Some(path) = settings_path.or(path_lookup) {
            path
        } else {
            download::verified_binary(host, &mut self.cached_binary)?
        };
        Ok(zed::Command {
            command: path,
            args: args.unwrap_or_else(|| vec!["server".into()]),
            env: vec![],
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
    /// Map Zed settings into the server settings envelope.
    /// Rejects malformed values with actionable errors.
    fn map_settings(lsp_settings: &LspSettings) -> Result<zed_extension_api::serde_json::Value> {
        let settings = lsp_settings
            .settings
            .clone()
            .unwrap_or_else(|| zed::serde_json::json!({}));

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

        // Zed selects the section requested by workspace/configuration.
        Ok(zed::serde_json::json!({ "ry": settings }))
    }
}

impl zed::Extension for RyExtension {
    fn new() -> Self {
        Self {
            cached_binary: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let args = binary.as_ref().and_then(|binary| binary.arguments.clone());
        self.command_with_host(
            &mut ZedReleaseHost(language_server_id),
            binary.and_then(|binary| binary.path),
            worktree.which("ry"),
            args,
        )
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
            None => Ok(Some(zed::serde_json::json!({ "ry": {} }))),
        }
    }
}

zed::register_extension!(RyExtension);

#[cfg(test)]
mod test {
    use crate::{GithubReleaseDetails, RyExtension};

    #[test]
    fn workspace_configuration_exposes_requested_ry_section() {
        use zed_extension_api::{serde_json, settings::LspSettings};
        for settings in [
            serde_json::json!({ "lint": { "ignore": ["RY040"] } }),
            serde_json::json!({}),
        ] {
            let lsp: LspSettings =
                serde_json::from_value(serde_json::json!({ "settings": settings })).unwrap();
            let configuration = RyExtension::map_settings(&lsp).unwrap();
            assert_eq!(configuration.get("ry"), Some(&settings));
        }
        let defaults: LspSettings = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(
            RyExtension::map_settings(&defaults).unwrap()["ry"],
            serde_json::json!({})
        );
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
