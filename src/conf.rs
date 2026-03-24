use anyhow::{bail, Result};

/// 加载 packages.conf 文件，返回包列表
/// 格式：package_list=(luna-pinyin terra-pinyin ...)
pub fn load_conf_file(content: &str) -> Result<Vec<String>> {
    let start = content.find("package_list=(");
    let Some(start) = start else {
        bail!("conf 文件中找不到 package_list");
    };
    let inner_start = start + "package_list=(".len();
    let Some(end) = content[inner_start..].find(')') else {
        bail!("package_list 未闭合");
    };
    let inner = &content[inner_start..inner_start + end];

    let packages: Vec<String> = inner
        .split_whitespace()
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(|s| s.to_string())
        .collect();

    Ok(packages)
}

/// 从 URL 下载 conf 文件内容
pub fn fetch_conf_url(url: &str) -> Result<String> {
    println!("获取配置列表: {}", url);
    let resp = ureq::get(url).call()?;
    let body = resp.into_body().read_to_string()?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_conf() {
        let content = r#"
package_list=(
  luna-pinyin
  terra-pinyin
  bopomofo
)
"#;
        let list = load_conf_file(content).unwrap();
        assert_eq!(list, vec!["luna-pinyin", "terra-pinyin", "bopomofo"]);
    }
}
