use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Debug, PartialEq)]
enum Statement {
    Set(Vec<String>),
    Append(Vec<String>),
    Source(String),
}

// Only parse plum's declarative subset; never run shell commands.
fn tokenize(content: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut chars = content.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '#' if word.is_empty() => {
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        break;
                    }
                }
                tokens.push("\n".into());
            }
            '\\' => {
                if let Some(next) = chars.next()
                    && next != '\n'
                {
                    word.push(next);
                }
            }
            '=' if word != "package_list" && word != "package_list+" && !word.is_empty() => {
                word.push(c)
            }
            '(' | ')' | '=' | ';' | '\n' => {
                if !word.is_empty() {
                    tokens.push(std::mem::take(&mut word));
                }
                tokens.push(if c == ';' { "\n".into() } else { c.to_string() });
            }
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    tokens.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if quote.is_some() {
        bail!("清单引号未闭合");
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    Ok(tokens)
}

fn parse(content: &str) -> Result<Vec<Statement>> {
    let tokens = tokenize(content)?;
    let mut i = 0;
    let mut statements = Vec::new();
    while i < tokens.len() {
        if tokens[i] == "\n" {
            i += 1;
            continue;
        }
        match tokens[i].as_str() {
            "package_list" | "package_list+" => {
                let append = tokens[i].ends_with('+');
                i += 1;
                if tokens.get(i).map(String::as_str) != Some("=")
                    || tokens.get(i + 1).map(String::as_str) != Some("(")
                {
                    bail!("非法 package_list 数组");
                }
                i += 2;
                let mut values = Vec::new();
                while i < tokens.len() && tokens[i] != ")" {
                    if tokens[i] != "\n" {
                        if tokens[i].contains(['$', '`', '(']) {
                            bail!("不支持清单中的 Shell 表达式: {}", tokens[i]);
                        }
                        values.push(tokens[i].clone());
                    }
                    i += 1;
                }
                if i == tokens.len() {
                    bail!("package_list 未闭合");
                }
                i += 1;
                statements.push(if append {
                    Statement::Append(values)
                } else {
                    Statement::Set(values)
                });
            }
            "source" | "." => {
                i += 1;
                let name = tokens
                    .get(i)
                    .filter(|s| s.as_str() != "\n")
                    .context("source 缺少文件名")?
                    .clone();
                statements.push(Statement::Source(name));
                i += 1;
            }
            other => bail!("不支持的清单语句: {other}"),
        }
    }
    if statements.is_empty() {
        bail!("conf 文件中找不到 package_list 或 source");
    }
    Ok(statements)
}

pub fn fetch_conf_url(url: &str) -> Result<String> {
    println!("获取配置列表: {url}");
    Ok(ureq::get(url).call()?.into_body().read_to_string()?)
}

pub fn builtin(name: &str) -> Option<&'static str> {
    match name {
        "preset-packages.conf" => Some(include_str!("../data/preset-packages.conf")),
        "extra-packages.conf" => Some(include_str!("../data/extra-packages.conf")),
        "all-packages.conf" => Some(include_str!("../data/all-packages.conf")),
        _ => None,
    }
}

pub fn load(location: &str, plum_dir: &Path) -> Result<Vec<String>> {
    let mut result = Vec::new();
    expand(location, plum_dir, &mut Vec::new(), &mut result)?;
    Ok(result)
}

fn expand(
    location: &str,
    plum_dir: &Path,
    stack: &mut Vec<String>,
    result: &mut Vec<String>,
) -> Result<()> {
    if stack.len() >= 32 || stack.iter().any(|s| s == location) {
        bail!("清单 source 循环或嵌套过深: {location}");
    }
    stack.push(location.into());
    let remote = location.starts_with("https://") || location.starts_with("http://");
    let path = Path::new(location);
    let content = if remote {
        fetch_conf_url(location)?
    } else if let Some(name) = location.strip_prefix("builtin:") {
        let local = crate::files::safe_join(plum_dir, name)?;
        if local.exists() {
            std::fs::read_to_string(local)?
        } else {
            builtin(name).context("未知内置清单")?.into()
        }
    } else {
        std::fs::read_to_string(path).with_context(|| format!("读取清单: {location}"))?
    };
    for statement in parse(&content)? {
        match statement {
            Statement::Set(values) => {
                result.clear();
                result.extend(values);
            }
            Statement::Append(values) => result.extend(values),
            Statement::Source(name) => {
                let mut name = name;
                for prefix in ["${root_dir:-.}/", "${root_dir}/", "$root_dir/"] {
                    if let Some(tail) = name.strip_prefix(prefix) {
                        name = tail.into();
                        break;
                    }
                }
                if name.contains(['$', '`']) {
                    bail!("不支持 source 表达式: {name}");
                }
                let next = if remote {
                    url::Url::parse(location)?.join(&name)?.to_string()
                } else if location.starts_with("builtin:") {
                    crate::files::safe_join(plum_dir, &name)?;
                    format!("builtin:{name}")
                } else {
                    crate::files::safe_join(path.parent().unwrap_or(Path::new(".")), &name)?
                        .to_string_lossy()
                        .into_owned()
                };
                expand(&next, plum_dir, stack, result)?;
            }
        }
    }
    stack.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_quotes_and_assignments() {
        assert_eq!(
            parse("package_list=(old)\npackage_list+=(\n 'new' # ignored words\n \"last\"\n)")
                .unwrap(),
            vec![
                Statement::Set(vec!["old".into()]),
                Statement::Append(vec!["new".into(), "last".into()])
            ]
        );
        assert!(parse("package_list=(bad").is_err());
        assert!(parse("echo unsafe").is_err());
    }
    #[test]
    fn nested_and_cycles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("preset-packages.conf"),
            "package_list+=(one)\n",
        )
        .unwrap();
        let all = load("builtin:all-packages.conf", dir.path()).unwrap();
        assert_eq!(all[0], "one");
        assert!(all.contains(&"wubi".into()));
        std::fs::write(dir.path().join("a.conf"), "source a.conf").unwrap();
        assert!(load(dir.path().join("a.conf").to_str().unwrap(), dir.path()).is_err());
    }
}
