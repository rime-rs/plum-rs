use anyhow::Result;
use std::io::{BufRead, Write};

pub fn select<R: BufRead, W: Write>(
    candidates: &[String],
    defaults: &[String],
    input: &mut R,
    output: &mut W,
) -> Result<Vec<String>> {
    for (i, name) in candidates.iter().enumerate() {
        writeln!(output, "{}) {name}", i + 1)?;
    }
    let mut selected = Vec::new();
    loop {
        write!(
            output,
            "输入编号、包名，或 . 完成（clear 重选，quit 取消）: "
        )?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        let answer = line.trim();
        match answer {
            "." | "end" | "ok" | "0" => break,
            "quit" | "cancel" | "exit" => return Ok(Vec::new()),
            "clear" | "reset" => {
                selected.clear();
                continue;
            }
            "" => continue,
            _ => {
                if let Ok(index) = answer.parse::<usize>() {
                    if let Some(name) = index.checked_sub(1).and_then(|i| candidates.get(i)) {
                        selected.push(name.clone());
                    } else {
                        writeln!(output, "无效编号: {answer}")?;
                    }
                } else if answer.starts_with(|c: char| c.is_ascii_alphabetic() || c == ':') {
                    selected.push(answer.into());
                } else {
                    writeln!(output, "无效包名: {answer}")?;
                }
            }
        }
    }
    if selected.is_empty() {
        write!(output, "未选择包，安装默认目标 {:?}？[Y/n] ", defaults)?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? > 0
            && (line.trim().is_empty() || line.trim().starts_with(['y', 'Y']))
        {
            return Ok(defaults.into());
        }
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_reset_numbers_names_and_cancel() {
        let names = vec!["one".into(), "two".into()];
        let selected = select(
            &names,
            &names,
            &mut &b"1\nclear\n2\ncustom/repo\n.\n"[..],
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(selected, vec!["two", "custom/repo"]);
        assert!(
            select(&names, &names, &mut &b"quit\n"[..], &mut Vec::new())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            select(&names, &names, &mut &b".\ny\n"[..], &mut Vec::new()).unwrap(),
            names
        );
    }
}
