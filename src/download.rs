use anyhow::{Context, Result};
use std::path::Path;

pub fn download(url: &str, path: &Path) -> Result<()> {
    let mut request = ureq::get(url);
    if let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) {
        request = request.header("If-Modified-Since", httpdate::fmt_http_date(modified));
    }
    let response = request.call().with_context(|| format!("下载失败: {url}"))?;
    if response.status() == 304 {
        return Ok(());
    }
    let modified = response
        .headers()
        .get("Last-Modified")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| httpdate::parse_http_date(s).ok());
    let bytes = response
        .into_body()
        .with_config()
        .limit(128 * 1024 * 1024)
        .read_to_vec()?;
    crate::files::atomic_write(path, &bytes)?;
    if let Some(modified) = modified {
        std::fs::File::options()
            .write(true)
            .open(path)?
            .set_modified(modified)?;
    }
    Ok(())
}
