use std::path::Path;

use zed_extension_api::{
    self as zed, settings::LspSettings, Architecture, DownloadedFileType,
    LanguageServerInstallationStatus, Os,
};

const REPOSITORY: &str = "KabaevRoman/inline-sql-zed";
const SERVER_NAME: &str = "inline-sql-lsp";

struct InlineSqlExtension {
    cached_server_path: Option<String>,
}

impl InlineSqlExtension {
    fn release_asset() -> zed::Result<(String, DownloadedFileType, String)> {
        let (os, architecture) = zed::current_platform();
        let (target, archive_type, executable) = match (os, architecture) {
            (Os::Mac, Architecture::Aarch64) => (
                "aarch64-apple-darwin",
                DownloadedFileType::GzipTar,
                SERVER_NAME,
            ),
            (Os::Mac, Architecture::X8664) => (
                "x86_64-apple-darwin",
                DownloadedFileType::GzipTar,
                SERVER_NAME,
            ),
            (Os::Linux, Architecture::Aarch64) => (
                "aarch64-unknown-linux-gnu",
                DownloadedFileType::GzipTar,
                SERVER_NAME,
            ),
            (Os::Linux, Architecture::X8664) => (
                "x86_64-unknown-linux-gnu",
                DownloadedFileType::GzipTar,
                SERVER_NAME,
            ),
            (Os::Windows, Architecture::X8664) => (
                "x86_64-pc-windows-msvc",
                DownloadedFileType::Zip,
                "inline-sql-lsp.exe",
            ),
            _ => return Err("Inline SQL does not provide a binary for this platform".into()),
        };
        let extension = match archive_type {
            DownloadedFileType::Zip => "zip",
            _ => "tar.gz",
        };
        Ok((
            format!("{SERVER_NAME}-{target}.{extension}"),
            archive_type,
            executable.into(),
        ))
    }

    fn installed_server_path(
        &mut self,
        language_server_id: &zed::LanguageServerId,
    ) -> zed::Result<String> {
        match self.download_server(language_server_id) {
            Ok(path) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &LanguageServerInstallationStatus::None,
                );
                Ok(path)
            }
            Err(error) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &LanguageServerInstallationStatus::Failed(error.clone()),
                );
                Err(error)
            }
        }
    }

    fn download_server(
        &mut self,
        language_server_id: &zed::LanguageServerId,
    ) -> zed::Result<String> {
        if let Some(path) = self.cached_server_path.as_ref() {
            if Path::new(path).is_file() {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        let release = zed::github_release_by_tag_name(REPOSITORY, &tag)
            .map_err(|error| format!("failed to find Inline SQL release {tag}: {error}"))?;
        let (asset_name, archive_type, executable) = Self::release_asset()?;
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("release {tag} has no asset named {asset_name}"))?;
        let install_dir = format!("inline-sql-lsp-{}", release.version);
        let server_path = format!("{install_dir}/{executable}");

        if !Path::new(&server_path).is_file() {
            zed::set_language_server_installation_status(
                language_server_id,
                &LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(&asset.download_url, &install_dir, archive_type)
                .map_err(|error| format!("failed to download {asset_name}: {error}"))?;
            if !matches!(zed::current_platform().0, Os::Windows) {
                zed::make_file_executable(&server_path)?;
            }
        }

        self.cached_server_path = Some(server_path.clone());
        Ok(server_path)
    }
}

impl zed::Extension for InlineSqlExtension {
    fn new() -> Self {
        Self {
            cached_server_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings = LspSettings::for_worktree("inline-sql", worktree)?;
        let binary = settings.binary.unwrap_or(zed::settings::CommandSettings {
            path: None,
            arguments: None,
            env: None,
        });

        let command = if let Some(path) = binary.path {
            path
        } else if let Some(path) = worktree.which(SERVER_NAME) {
            path
        } else {
            self.installed_server_path(language_server_id)?
        };

        Ok(zed::Command {
            command,
            args: binary.arguments.unwrap_or_default(),
            env: binary
                .env
                .map(|env| env.into_iter().collect())
                .unwrap_or_else(|| worktree.shell_env()),
        })
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree("inline-sql", worktree)?.settings)
    }
}

zed::register_extension!(InlineSqlExtension);
