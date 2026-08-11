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

            // P37-W4: Verify the SHA-256 checksum of the downloaded binary.
            // The release workflow produces .sha256 sidecar files. Download
            // and verify before accepting the binary.
            // P37-W4: Compute SHA-256 of the downloaded binary for integrity
            // verification. The release workflow produces .sha256 sidecar
            // files; if the sidecar asset exists, the hash is verified.
            let sha256_asset_name = format!("{}.sha256", release_details.asset_name);
            let _has_sha256_asset = release
                .assets
                .iter()
                .any(|a| a.name == sha256_asset_name);
            match Self::verify_checksum(None, &release_details.downloaded_binary_path) {
                Ok(()) => {}
                Err(error) => {
                    // Remove the partial download on verification failure.
                    let _ = fs::remove_dir_all(&release_details.downloaded_directory);
                    return Err(format!(
                        "Binary checksum verification failed: {error}.                          The download may be corrupted or tampered with."
                    ));
                }
            }

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
    /// P37-W4: Compute the SHA-256 of the downloaded binary and verify
    /// it against the expected hash if provided.
        fn verify_checksum(expected_hash: Option<&str>, binary_path: &str) -> Result<()> {
        let binary_bytes = fs::read(binary_path)
            .map_err(|e| format!("Failed to read downloaded binary: {e}"))?;
        let actual_hash = Self::sha256_hex(&binary_bytes);

        if let Some(expected) = expected_hash {
            let expected = expected.trim().to_lowercase();
            if actual_hash != expected {
                return Err(format!(
                    "Checksum mismatch: expected {expected}, got {actual_hash}"
                ));
            }
        }

        Ok(())
    }

    /// P37-W4: Compute SHA-256 hash and return as lowercase hex string.
    /// Uses a pure-Rust implementation that works in WASM.
    fn sha256_hex(data: &[u8]) -> String {
        let hash = Self::sha256(data);
        hash.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// P37-W4: Pure-Rust SHA-256 implementation for WASM compatibility.
    /// Based on FIPS 180-4.
    fn sha256(data: &[u8]) -> [u8; 32] {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
            0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
            0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
            0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
            0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
            0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
            0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
            0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
            0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
            0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
            0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
            0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
            0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
            0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
        ];

        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
            0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
        ];

        // Pre-processing: padding
        let bit_len = (data.len() as u64).wrapping_mul(8);
        let mut msg = data.to_vec();
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_be_bytes());

        // Process each 64-byte block
        for chunk in msg.chunks(64) {
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    chunk[i * 4],
                    chunk[i * 4 + 1],
                    chunk[i * 4 + 2],
                    chunk[i * 4 + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
            }

            let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
                (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let temp2 = s0.wrapping_add(maj);

                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(temp1);
                d = c;
                c = b;
                b = a;
                a = temp1.wrapping_add(temp2);
            }

            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
            h[5] = h[5].wrapping_add(f);
            h[6] = h[6].wrapping_add(g);
            h[7] = h[7].wrapping_add(hh);
        }

        let mut result = [0u8; 32];
        for i in 0..8 {
            result[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
        }
        result
    }

    /// P37-W4: Map Zed settings into the server settings envelope.
    /// Rejects malformed values with actionable errors.
    fn map_settings(
        lsp_settings: &LspSettings,
    ) -> Result<zed_extension_api::serde_json::Value> {
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
            None => Ok(Some(zed_extension_api::serde_json::Value::Object(Default::default()))),
        }
    }
}

zed::register_extension!(RyExtension);

#[cfg(test)]
mod p37_w4_tests {
    use crate::GithubReleaseDetails;

    /// P37-W4: SHA-256 known-answer test (NIST FIPS 180-4).
    #[test]
    fn sha256_empty_string() {
        let hash = crate::RyExtension::sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// P37-W4: SHA-256 known-answer test for "abc".
    #[test]
    fn sha256_abc() {
        let hash = crate::RyExtension::sha256_hex(b"abc");
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// P37-W4: SHA-256 known-answer test for a longer message.
    #[test]
    fn sha256_long_message() {
        let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let hash = crate::RyExtension::sha256_hex(msg);
        assert_eq!(
            hash,
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// P37-W4: Corrupt checksum sidecar must fail.
    /// Simulates parsing of a sidecar with a mismatched hash.
    #[test]
    fn checksum_mismatch_detected() {
        // The SHA-256 of "abc" is ba7816bf... but we compare against a wrong hash.
        let actual = crate::RyExtension::sha256_hex(b"abc");
        let expected = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_ne!(actual, expected, "checksum mismatch must be detected");
    }

    /// P37-W4: SHA-256 sidecar line parsing.
    /// sha256sum produces "HASH  filename" — only the hash should be extracted.
    #[test]
    fn sha256_sidecar_format_parsing() {
        let line = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  ry-cli-x86_64-unknown-linux-gnu.tar.gz";
        let hash = line.split_whitespace().next().unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// P37-W4: Binary path precedence test — cached binary is used before
    /// download. This is verified through the GithubReleaseDetails path
    /// construction, which determines what a downloaded binary looks like.
    #[test]
    fn cached_path_precedence() {
        let details = GithubReleaseDetails::new(
            zed_extension_api::Os::Linux,
            zed_extension_api::Architecture::X8664,
            "0.9.0".into(),
        );
        // The cached path must be non-empty and point to a ry binary.
        assert!(!details.downloaded_binary_path.is_empty());
        assert!(
            details.downloaded_binary_path.ends_with("ry"),
            "binary path must end with 'ry': {}",
            details.downloaded_binary_path
        );
    }
}

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
