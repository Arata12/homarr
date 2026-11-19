use std::path::{Path, PathBuf};

use homarr_common::{HomarrError, HomarrResult};
use reqwest::Certificate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateFileInfo {
    pub file_name: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct CertificateManager {
    dir: PathBuf,
}

impl CertificateManager {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn sanitize_file_name(file_name: &str) -> HomarrResult<&str> {
        let trimmed = file_name.trim();
        if trimmed.is_empty()
            || trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.contains("..")
            || !(trimmed.ends_with(".crt") || trimmed.ends_with(".pem"))
        {
            return Err(HomarrError::BadRequest(
                "Certificate filename must be a simple .crt or .pem name without path separators"
                    .to_string(),
            ));
        }
        Ok(trimmed)
    }

    pub fn list(&self) -> HomarrResult<Vec<CertificateFileInfo>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        let entries = std::fs::read_dir(&self.dir).map_err(|err| {
            HomarrError::Internal(format!("Failed to read certificate dir: {err}"))
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !(name.ends_with(".crt") || name.ends_with(".pem")) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                results.push(CertificateFileInfo {
                    file_name: name.to_string(),
                    content,
                });
            }
        }
        results.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        Ok(results)
    }

    pub fn save(&self, file_name: &str, pem_content: &str) -> HomarrResult<()> {
        let clean = Self::sanitize_file_name(file_name)?;
        if !pem_content.contains("-----BEGIN CERTIFICATE-----") {
            return Err(HomarrError::BadRequest(
                "Certificate must be PEM encoded (-----BEGIN CERTIFICATE-----)".to_string(),
            ));
        }
        // Validate that reqwest/rustls can parse the PEM
        Certificate::from_pem(pem_content.as_bytes())
            .map_err(|err| HomarrError::BadRequest(format!("Invalid PEM certificate: {err}")))?;

        std::fs::create_dir_all(&self.dir)
            .map_err(|err| HomarrError::Internal(format!("Failed to create cert dir: {err}")))?;
        std::fs::write(self.dir.join(clean), pem_content)
            .map_err(|err| HomarrError::Internal(format!("Failed to write cert file: {err}")))?;
        Ok(())
    }

    pub fn remove(&self, file_name: &str) -> HomarrResult<()> {
        let clean = Self::sanitize_file_name(file_name)?;
        let path = self.dir.join(clean);
        if !path.exists() {
            return Err(HomarrError::NotFound(format!(
                "Certificate '{clean}' not found"
            )));
        }
        std::fs::remove_file(path)
            .map_err(|err| HomarrError::Internal(format!("Failed to remove cert: {err}")))?;
        Ok(())
    }

    pub fn load_reqwest_certificates(&self) -> Vec<Certificate> {
        let Ok(list) = self.list() else {
            return Vec::new();
        };
        list.into_iter()
            .filter_map(|info| Certificate::from_pem(info.content.as_bytes()).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_filenames() {
        let mgr = CertificateManager::new(PathBuf::from("./test-certs-nonexistent"));
        assert!(mgr
            .save("../evil.pem", "-----BEGIN CERTIFICATE-----")
            .is_err());
        assert!(mgr
            .save("sub/cert.crt", "-----BEGIN CERTIFICATE-----")
            .is_err());
        assert!(mgr.save("cert.txt", "-----BEGIN CERTIFICATE-----").is_err());
        assert!(mgr.list().unwrap().is_empty());
    }
}
