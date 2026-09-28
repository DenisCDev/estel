//! Checks published Estel releases and verifies the installer before opening it.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ring::digest::{Context, SHA256};
use serde::Deserialize;

const RELEASE_URL: &str = "https://api.github.com/repos/DenisCDev/estel/releases/latest";
const INSTALLER_NAME: &str = "Estel-Setup-x86_64.exe";
const MAX_INSTALLER_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    url: String,
    pub size: u64,
    digest: String,
}

pub struct DownloadedInstaller(Option<PathBuf>);

impl Drop for DownloadedInstaller {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            discard_installer(&path);
        }
    }
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(15))
        .user_agent(concat!("Estel/", env!("CARGO_PKG_VERSION")))
        .build()
}

pub fn check_latest() -> Result<Option<Release>, String> {
    cleanup_old_installers();
    let response = agent()
        .get(RELEASE_URL)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| {
            tracing::warn!(%error, "consulta de atualização indisponível");
            "Não foi possível consultar as atualizações. Confira a conexão e tente novamente."
                .to_owned()
        })?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take(65_537)
        .read_to_end(&mut body)
        .map_err(|_| "Não foi possível ler a resposta do GitHub.".to_owned())?;
    if body.len() > 65_536 {
        return Err("A resposta de atualização excedeu o tamanho esperado.".into());
    }
    let release: ApiRelease = serde_json::from_slice(&body)
        .map_err(|_| "O GitHub enviou dados de atualização inválidos.".to_owned())?;
    release_from_api(release)
}

fn version_parts(version: &str) -> Option<[u32; 3]> {
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    let numbers = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(numbers)
}

fn release_from_api(api: ApiRelease) -> Result<Option<Release>, String> {
    let latest = version_parts(&api.tag_name)
        .ok_or_else(|| "O GitHub publicou uma versão com formato desconhecido.".to_owned())?;
    let current = version_parts(env!("CARGO_PKG_VERSION"))
        .ok_or_else(|| "A versão instalada tem formato desconhecido.".to_owned())?;
    if latest <= current {
        return Ok(None);
    }
    let asset = api
        .assets
        .into_iter()
        .find(|asset| asset.name == INSTALLER_NAME)
        .ok_or_else(|| "A nova versão ainda não tem instalador para Windows.".to_owned())?;
    let expected_url = format!(
        "https://github.com/DenisCDev/estel/releases/download/{}/{INSTALLER_NAME}",
        api.tag_name
    );
    if asset.browser_download_url != expected_url
        || asset.size == 0
        || asset.size > MAX_INSTALLER_BYTES
    {
        return Err("O instalador publicado não passou na verificação de origem e tamanho.".into());
    }
    let digest = asset
        .digest
        .and_then(|value| value.strip_prefix("sha256:").map(str::to_owned))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "O instalador publicado não tem SHA-256 válido.".to_owned())?;
    Ok(Some(Release {
        version: api.tag_name,
        url: expected_url,
        size: asset.size,
        digest,
    }))
}

pub fn download_installer(
    release: &Release,
    downloaded: &AtomicU64,
) -> Result<DownloadedInstaller, String> {
    let response = agent().get(&release.url).call().map_err(|error| {
        tracing::warn!(%error, "download da atualização falhou");
        "Não foi possível baixar a atualização. Confira a conexão e tente novamente.".to_owned()
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Não foi possível preparar o download.".to_owned())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("Estel-Setup-{}-{stamp}.exe", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "Não foi possível criar o arquivo temporário da atualização.".to_owned())?;
    let result = (|| {
        let mut reader = response.into_reader();
        let mut hash = Context::new(&SHA256);
        let mut buffer = [0_u8; 64 * 1024];
        let mut total = 0_u64;
        loop {
            let count = reader.read(&mut buffer).map_err(|error| {
                tracing::warn!(%error, "download da atualização interrompido");
                "O download foi interrompido. Tente novamente.".to_owned()
            })?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > release.size {
                return Err("O instalador baixado excedeu o tamanho publicado.".into());
            }
            hash.update(&buffer[..count]);
            file.write_all(&buffer[..count])
                .map_err(|_| "Não foi possível gravar o instalador temporário.".to_owned())?;
            downloaded.store(total, Ordering::Relaxed);
        }
        let actual_digest = hash
            .finish()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if total != release.size || actual_digest != release.digest {
            return Err("O instalador baixado não corresponde ao SHA-256 publicado.".into());
        }
        file.flush()
            .map_err(|_| "Não foi possível concluir o arquivo temporário.".to_owned())?;
        Ok(())
    })();
    drop(file);
    if let Err(error) = result {
        if let Err(cleanup_error) = fs::remove_file(&path) {
            tracing::warn!(%cleanup_error, "não foi possível limpar o download incompleto");
        }
        return Err(error);
    }
    Ok(DownloadedInstaller(Some(path)))
}

fn discard_installer(path: &PathBuf) {
    if let Err(error) = fs::remove_file(path) {
        tracing::warn!(%error, file = %path.display(), "não foi possível limpar o instalador temporário");
    }
}

fn cleanup_old_installers() {
    let entries = match fs::read_dir(std::env::temp_dir()) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, "não foi possível verificar instaladores temporários");
            return;
        }
    };
    for entry in entries.take(4096).flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name
            .strip_prefix("Estel-Setup-")
            .and_then(|value| value.strip_suffix(".exe"))
        else {
            continue;
        };
        let Some((pid, stamp)) = id.split_once('-') else {
            continue;
        };
        if !pid.bytes().all(|byte| byte.is_ascii_digit())
            || !stamp.bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let old_enough = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age >= Duration::from_secs(5 * 60));
        if old_enough {
            discard_installer(&entry.path());
        }
    }
}

pub fn launch_installer(mut installer: DownloadedInstaller) -> Result<(), String> {
    let path = installer.0.take().expect("instalador ainda não foi aberto");
    let mut child = match Command::new(&path).spawn() {
        Ok(child) => child,
        Err(error) => {
            discard_installer(&path);
            return Err(format!("Não foi possível abrir o instalador ({error})."));
        }
    };
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30 * 60);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    discard_installer(&path);
                    return;
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_secs(1)),
                Ok(None) => {
                    tracing::warn!(
                        "o instalador ainda está aberto; o arquivo temporário será mantido"
                    );
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "não foi possível acompanhar o instalador");
                    return;
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(version_parts("v0.2.10") > version_parts("0.2.9"));
        assert_eq!(version_parts("v0.2.4"), Some([0, 2, 4]));
        assert_eq!(version_parts("v0.2.4-beta"), None);
    }

    #[test]
    fn update_requires_expected_installer_and_digest() {
        let json = format!(
            r#"{{"tag_name":"v99.0.0","assets":[{{"name":"{INSTALLER_NAME}","size":5,"digest":"sha256:{}","browser_download_url":"https://github.com/DenisCDev/estel/releases/download/v99.0.0/{INSTALLER_NAME}"}}]}}"#,
            "a".repeat(64)
        );
        let release: ApiRelease = serde_json::from_str(&json).unwrap();
        assert!(release_from_api(release).unwrap().is_some());
        let untrusted = json.replace("github.com", "example.com");
        let release: ApiRelease = serde_json::from_str(&untrusted).unwrap();
        assert!(release_from_api(release).is_err());
    }

    #[test]
    #[ignore = "baixa o instalador publicado de aproximadamente 9 MB"]
    fn published_installer_matches_github_digest() {
        let release = Release {
            version: "v0.2.3".into(),
            url:
                "https://github.com/DenisCDev/estel/releases/download/v0.2.3/Estel-Setup-x86_64.exe"
                    .into(),
            size: 9_073_149,
            digest: "99d868328d7a86afefabf6fed628044c6e0206fb1917e7b3fc947790c25ddc52".into(),
        };
        let downloaded = AtomicU64::new(0);
        let installer = download_installer(&release, &downloaded).unwrap();
        assert_eq!(downloaded.load(Ordering::Relaxed), release.size);
        let path = installer.0.as_ref().unwrap().clone();
        assert_eq!(fs::metadata(&path).unwrap().len(), release.size);
        drop(installer);
        assert!(!path.exists());
    }
}
