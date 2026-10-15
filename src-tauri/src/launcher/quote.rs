//! 命令行引号处理。
//!
//! 只对**程序路径**按 MSVCRT 规则加引号；**参数原样透传**，不做分词、不重新转义（ADR-03）。
//! 之所以把规则写全，是因为「路径里有空格」在 Windows 上非常常见，
//! 而引号加错会导致程序收到被拆碎的 argv[0]。

/// 按 MSVCRT 规则给单个参数加引号。
///
/// 规则（与 CRT 的 argv 解析互逆）：
/// - 不含空格 / 制表符 / 引号 → 原样返回；
/// - 否则用 `"` 包住字符串；
/// - 「连续 n 个反斜杠 + 引号」要写成 `2n+1` 个反斜杠再加引号；
/// - 结尾的连续反斜杠要加倍成 `2n`，否则它会转义收尾的那个引号。
pub fn quote_argument(argument: &str) -> String {
    if !needs_quoting(argument) {
        return argument.to_owned();
    }

    let mut out = String::with_capacity(argument.len() + 2);
    out.push('"');

    let mut pending_backslashes = 0usize;
    for ch in argument.chars() {
        match ch {
            '\\' => pending_backslashes += 1,
            '"' => {
                // 反斜杠加倍，再补一个转义引号本身
                push_repeated(&mut out, '\\', pending_backslashes * 2 + 1);
                out.push('"');
                pending_backslashes = 0;
            }
            _ => {
                push_repeated(&mut out, '\\', pending_backslashes);
                pending_backslashes = 0;
                out.push(ch);
            }
        }
    }

    // 结尾反斜杠加倍，避免转义收尾引号
    push_repeated(&mut out, '\\', pending_backslashes * 2);
    out.push('"');

    out
}

/// 给程序路径加引号。这就是 `lpCommandLine` 里出现在最前面的那一部分。
pub fn quote_program_path(path: &str) -> String {
    quote_argument(path)
}

/// 拼接 `lpCommandLine`：引号包裹的程序路径 + 原样参数串。
///
/// 参数若为空则只留路径。**不对参数做任何加工**。
pub fn build_command_line(program_path: &str, raw_args: &str) -> String {
    let quoted_program = quote_program_path(program_path);
    let args = raw_args.trim();

    if args.is_empty() {
        quoted_program
    } else {
        format!("{quoted_program} {args}")
    }
}

/// 需要加引号的情况：空串，或含空格 / 制表符 / 引号。
fn needs_quoting(argument: &str) -> bool {
    argument.is_empty() || argument.contains([' ', '\t', '"'])
}

fn push_repeated(out: &mut String, ch: char, times: usize) {
    for _ in 0..times {
        out.push(ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_simple_arguments_untouched() {
        assert_eq!(quote_argument("simple"), "simple");
        assert_eq!(quote_argument(r"C:\Games\demo.exe"), r"C:\Games\demo.exe");
        assert_eq!(quote_argument("中文参数"), "中文参数");
    }

    #[test]
    fn quotes_arguments_with_spaces() {
        assert_eq!(quote_argument("a b"), r#""a b""#);
        assert_eq!(
            quote_argument(r"C:\Program Files\Game\game.exe"),
            r#""C:\Program Files\Game\game.exe""#
        );
        assert_eq!(quote_argument("a\tb"), "\"a\tb\"");
    }

    #[test]
    fn escapes_quotes_with_backslashes() {
        // 无反斜杠 + 引号 → 1 个反斜杠转义
        assert_eq!(quote_argument(r#"a"b"#), r#""a\"b""#);
        // 1 个反斜杠 + 引号 → 3 个反斜杠
        assert_eq!(quote_argument(r#"a\"b"#), r#""a\\\"b""#);
        // 2 个反斜杠 + 引号 → 5 个反斜杠
        assert_eq!(quote_argument(r#"a\\"b"#), r#""a\\\\\"b""#);
    }

    #[test]
    fn doubles_trailing_backslashes_when_quoted() {
        // 结尾 2 个反斜杠 + 需要引号 → 4 个反斜杠再收尾引号
        assert_eq!(quote_argument("a b\\\\"), r#""a b\\\\""#);
        // 不以空格结尾时不需要引号，反斜杠也不动
        assert_eq!(quote_argument(r"a\b\\"), r"a\b\\");
    }

    #[test]
    fn builds_command_line_without_touching_args() {
        let line = build_command_line(
            r"C:\Games\My Game\game.exe",
            r#"-windowed "--save=C:\My Saves\slot 1""#,
        );
        assert_eq!(
            line, r#""C:\Games\My Game\game.exe" -windowed "--save=C:\My Saves\slot 1""#,
            "参数部分必须逐字保留"
        );
    }

    #[test]
    fn builds_command_line_with_empty_args() {
        assert_eq!(
            build_command_line(r"C:\a\b.exe", "   "),
            r"C:\a\b.exe",
            "空参数不应留下多余空格"
        );
    }
}
