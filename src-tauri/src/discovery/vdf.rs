//! 极简文本 VDF 解析器（Valve Data Format）。
//!
//! 只实现 Steam 两个文件（`libraryfolders.vdf` / `appmanifest_*.acf`）用到的子集：
//! 引号键值对、`{}` 嵌套、`\\"` 与 `\\\\` 转义、`//` 注释、无引号的裸值。
//! **不做**转义还原成反斜杠以外的花活 —— Steam 文件里路径写的是 `D:\\steam`，
//! 解析后应得到 `D:\steam`。

/// 解析结果。用 `Vec` 而不是 `HashMap`：VDF 允许重复键，且保序便于调试。
#[derive(Debug, Clone, PartialEq)]
pub enum VdfValue {
    Str(String),
    Table(Vec<(String, VdfValue)>),
}

impl VdfValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            VdfValue::Str(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&[(String, VdfValue)]> {
        match self {
            VdfValue::Table(pairs) => Some(pairs),
            _ => None,
        }
    }
}

/// 在一层键值对里取字符串值。
pub fn get_str<'a>(pairs: &'a [(String, VdfValue)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.as_str())
}

/// 在一层键值对里取嵌套表。
pub fn get_table<'a>(
    pairs: &'a [(String, VdfValue)],
    key: &str,
) -> Option<&'a [(String, VdfValue)]> {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.as_table())
}

/// 解析整段 VDF 文本。
pub fn parse(input: &str) -> Result<Vec<(String, VdfValue)>, String> {
    let mut parser = Parser {
        chars: input.chars().peekable(),
    };
    parser.parse_pairs(/* nested */ false)
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Parser<'_> {
    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn bump(&mut self) -> Option<char> {
        self.chars.next()
    }

    fn skip_blank(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.bump();
            } else if ch == '/' && {
                // 注释：`//` 到行尾
                let mut clone = self.chars.clone();
                clone.next();
                clone.next() == Some('/')
            } {
                while let Some(ch) = self.bump() {
                    if ch == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// `nested = true` 表示处于 `{}` 内部，遇到 `}` 就返回。
    fn parse_pairs(&mut self, nested: bool) -> Result<Vec<(String, VdfValue)>, String> {
        let mut pairs = Vec::new();

        loop {
            self.skip_blank();
            let Some(ch) = self.peek() else {
                if nested {
                    return Err("嵌套块缺少收尾的 }".to_owned());
                }
                return Ok(pairs);
            };

            if ch == '}' {
                if nested {
                    self.bump();
                    return Ok(pairs);
                }
                return Err("多余的 }".to_owned());
            }

            let key = self.read_token()?;
            self.skip_blank();

            let value = if self.peek() == Some('{') {
                self.bump();
                let inner = self.parse_pairs(true)?;
                VdfValue::Table(inner)
            } else {
                VdfValue::Str(self.read_token()?)
            };

            pairs.push((key, value));
        }
    }

    /// 读一个「引号内」或「裸」的 token。
    fn read_token(&mut self) -> Result<String, String> {
        self.skip_blank();
        match self.peek() {
            Some('"') => self.read_quoted(),
            Some(ch) if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' => {
                let mut out = String::new();
                while let Some(ch) = self.peek() {
                    if ch.is_whitespace() || ch == '{' || ch == '}' {
                        break;
                    }
                    out.push(self.bump().expect("peek 过的字符必然存在"));
                }
                Ok(out)
            }
            other => Err(format!("非法字符：{other:?}")),
        }
    }

    fn read_quoted(&mut self) -> Result<String, String> {
        // 消耗开头的引号
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err("引号未闭合".to_owned()),
                Some('"') => return Ok(out),
                Some('\\') => match self.bump() {
                    None => return Err("转义符后没有字符".to_owned()),
                    Some(esc) => {
                        // Steam 只用这几种转义；未知的按字面保留
                        match esc {
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            '"' => out.push('"'),
                            '\\' => out.push('\\'),
                            other => {
                                out.push('\\');
                                out.push(other);
                            }
                        }
                    }
                },
                Some(ch) => out.push(ch),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_tables_and_escaped_paths() {
        let text = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"730\"\t\t\"1\"\n\t\t}\n\t}\n}";
        let parsed = parse(text).expect("解析");
        let root = get_table(&parsed, "libraryfolders").expect("根表");
        let zero = get_table(root, "0").expect("第一个库");
        assert_eq!(
            get_str(zero, "path"),
            Some("D:\\steam"),
            "双反斜杠应还原为单个"
        );
        let apps = get_table(zero, "apps").expect("apps 表");
        assert_eq!(get_str(apps, "730"), Some("1"));
    }

    #[test]
    fn parses_manifest_appstate() {
        let text = "\"AppState\"\n{\n\t\"appid\"\t\t\"292030\"\n\t\"name\"\t\t\"The Witcher 3: Wild Hunt — Remastered\"\n\t\"StateFlags\"\t\t\"4\"\n}";
        let parsed = parse(text).expect("解析");
        let state = get_table(&parsed, "AppState").expect("AppState");
        assert_eq!(get_str(state, "appid"), Some("292030"));
        assert_eq!(
            get_str(state, "name"),
            Some("The Witcher 3: Wild Hunt — Remastered"),
            "Unicode 名应原样保留"
        );
        assert_eq!(get_str(state, "StateFlags"), Some("4"));
    }

    #[test]
    fn tolerates_comments_and_bare_values() {
        let text = "\"root\"\n{\n\t// 这是注释\n\tkey value\n\t\"quoted\" \"value\"\n}";
        let parsed = parse(text).expect("解析");
        let root = get_table(&parsed, "root").expect("根表");
        assert_eq!(get_str(root, "key"), Some("value"));
        assert_eq!(get_str(root, "quoted"), Some("value"));
    }

    #[test]
    fn rejects_unbalanced_input() {
        assert!(parse("{").is_err(), "缺收尾括号应报错");
        assert!(parse("\"a\" \"b\"\n}").is_err(), "多余括号应报错");
        assert!(parse("\"unterminated").is_err(), "引号未闭合应报错");
    }
}
