//! Battle.net 来源：解析 `%PROGRAMDATA%\Battle.net\Agent\product.db`（protobuf）。
//!
//! 格式结论来自 2026-10-16 阶段一调查（docs/auto-discovery-design.md §10.3）：
//! 顶层 field 1 = Product（repeated）；Product 内 field 2 = 产品代号（string）、
//! field 3 = 子消息 { field 1 = base_path（string）、field 2 = region（string）}；
//! 其余字段未识别一律忽略。字段号无官方 schema 保证（参考 Lutris 实现 + 本机单样本），
//! 遇 Battle.net 大版本更新需重新验证。
//!
//! 只读：不启动 Battle.net、不读账号数据、不修改任何平台文件。

use std::path::{Path, PathBuf};

use super::{DiscoveredGame, DiscoveryConfidence, DiscoverySource, SourceScanResult, SourceState};

/// 产品代号 → （显示名, 安装目录下的已知启动 EXE）。
///
/// 仅收录公开可知且可信的代号；未收录的代号不会产生候选（计入 skipped，
/// 不伪造路径）。`hsb` 已在本机实机验证，其余标注「公开资料，未实机验证」。
const KNOWN_GAMES: &[(&str, &str, &str)] = &[
    ("hsb", "炉石传说", "Hearthstone.exe"),
    ("hs", "炉石传说", "Hearthstone.exe"),
    ("wow", "魔兽世界", "Wow.exe"),
    ("d3", "暗黑破坏神 III", "Diablo III.exe"),
    ("ow", "守望先锋", "Overwatch.exe"),
    ("s1", "暗黑破坏神 II：重制版", "D2R.exe"),
    ("w3", "魔兽争霸 III：重制版", "Warc3.exe"),
];

/// 启动器 / 后台组件的产品代号：不是游戏，一律排除。
const LAUNCHER_CODES: &[&str] = &["agent", "bna", "bnt"];

/// 一条解析出的产品记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductEntry {
    pub code: String,
    pub install_path: String,
    pub region: Option<String>,
}

/// protobuf wire format 的一个字段值。
#[derive(Debug, Clone)]
enum FieldValue {
    /// 数值字段：解析器不需要其语义，但必须跳过以保持 framing 正确。
    #[allow(dead_code)]
    Varint(u64),
    Bytes(Vec<u8>),
}

/// 严格解析一段 protobuf：成功返回字段列表，格式错误返回 None。
fn parse_fields(buf: &[u8]) -> Option<Vec<(u32, FieldValue)>> {
    let mut fields = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let field = (tag >> 3) as u32;
        let wire = (tag & 7) as u32;
        let value = match wire {
            0 => FieldValue::Varint(read_varint(buf, &mut pos)?),
            2 => {
                let len = read_varint(buf, &mut pos)? as usize;
                if pos + len > buf.len() {
                    return None; // 截断
                }
                let bytes = buf[pos..pos + len].to_vec();
                pos += len;
                FieldValue::Bytes(bytes)
            }
            // wire 1/5 固定长度：跳过但不解释
            1 => {
                if pos + 8 > buf.len() {
                    return None;
                }
                let bytes = buf[pos..pos + 8].to_vec();
                pos += 8;
                FieldValue::Bytes(bytes)
            }
            5 => {
                if pos + 4 > buf.len() {
                    return None;
                }
                let bytes = buf[pos..pos + 4].to_vec();
                pos += 4;
                FieldValue::Bytes(bytes)
            }
            // wire 3/4（组）已废弃，出现即视为格式不受支持
            _ => return None,
        };
        fields.push((field, value));
    }
    Some(fields)
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    let mut shift = 0u32;
    while *pos < buf.len() {
        let byte = buf[*pos];
        *pos += 1;
        result |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
        if shift > 63 {
            return None; // varint 过长，防御
        }
    }
    None
}

fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// 解析 product.db 的全部内容。返回（有效记录，损坏/跳过的记录数）。
pub fn parse_product_db(data: &[u8]) -> (Vec<ProductEntry>, usize) {
    let mut entries = Vec::new();
    let mut skipped = 0usize;
    let Some(fields) = parse_fields(data) else {
        return (entries, 1); // 整体不是合法 protobuf
    };
    for (field, value) in fields {
        if field != 1 {
            continue; // 顶层级未识别字段（如 5/6/7）一律忽略
        }
        let FieldValue::Bytes(message) = value else {
            skipped += 1;
            continue;
        };
        match parse_product(&message) {
            Some(entry) => entries.push(entry),
            None => skipped += 1, // 单条损坏只跳过这一条
        }
    }
    (entries, skipped)
}

fn parse_product(message: &[u8]) -> Option<ProductEntry> {
    let fields = parse_fields(message)?;
    let mut code = None;
    let mut install_path = None;
    let mut region = None;
    for (field, value) in fields {
        match (field, value) {
            (2, FieldValue::Bytes(bytes)) => code = Some(decode(&bytes)),
            (3, FieldValue::Bytes(sub)) => {
                for (sub_field, sub_value) in parse_fields(&sub)? {
                    match (sub_field, sub_value) {
                        (1, FieldValue::Bytes(bytes)) => install_path = Some(decode(&bytes)),
                        (2, FieldValue::Bytes(bytes)) => region = Some(decode(&bytes)),
                        _ => {} // 子消息未识别字段
                    }
                }
            }
            _ => {} // uid / 版本串等未识别字段
        }
    }
    let code = code.filter(|value| !value.is_empty())?;
    let install_path = install_path.filter(|value| !value.is_empty())?;
    Some(ProductEntry {
        code,
        install_path,
        region,
    })
}

fn default_product_db() -> Option<PathBuf> {
    let program_data = std::env::var("PROGRAMDATA").ok()?;
    let path = Path::new(&program_data).join(r"Battle.net\Agent\product.db");
    path.is_file().then_some(path)
}

/// 扫描 Battle.net 来源。
///
/// `db_path` 传 `None` 时使用默认位置；测试传脱敏 fixture 路径。
pub fn scan_battlenet(db_path: Option<&Path>) -> SourceScanResult {
    let Some(path) = db_path.map(Path::to_path_buf).or_else(default_product_db) else {
        // 未安装 Battle.net（Agent 目录不存在）
        return SourceScanResult::not_installed(DiscoverySource::Battlenet);
    };
    if !path.is_file() {
        // 显式路径或默认路径不存在 → 未安装（区别于「存在但读不了」的 Unavailable）
        return SourceScanResult::not_installed(DiscoverySource::Battlenet);
    }
    let Ok(data) = std::fs::read(&path) else {
        return SourceScanResult::unavailable(
            DiscoverySource::Battlenet,
            format!("无法读取产品数据库 {}", path.display()),
        );
    };
    if data.is_empty() {
        // 文件存在但为空：客户端在、无任何产品记录
        return SourceScanResult {
            source: DiscoverySource::Battlenet,
            state: SourceState::Available,
            candidates: Vec::new(),
            skipped: 0,
        };
    }

    let (entries, parse_skipped) = parse_product_db(&data);
    let mut candidates = Vec::new();
    let mut skipped = parse_skipped;

    for entry in entries {
        if LAUNCHER_CODES.contains(&entry.code.as_str()) {
            continue; // agent / bna / bnt 是启动器与后台组件
        }
        // 未收录的产品代号：无法给出可靠的显示名与启动 EXE，不伪造
        let Some((display_name, known_exe)) = KNOWN_GAMES
            .iter()
            .find(|(code, _, _)| *code == entry.code)
            .map(|(_, n, e)| (*n, *e))
        else {
            skipped += 1;
            continue;
        };

        let install_dir = PathBuf::from(entry.install_path.replace('/', r"\"));
        let launch_path = install_dir.join(known_exe);
        if !launch_path.is_file() {
            // 卸载残留 / 未下载完整：安装记录在，游戏文件没了
            skipped += 1;
            continue;
        }

        candidates.push(DiscoveredGame {
            name: display_name.to_owned(),
            launch_path: launch_path.to_string_lossy().into_owned(),
            launch_args: None,
            working_directory: Some(install_dir.to_string_lossy().into_owned()),
            sources: vec![DiscoverySource::Battlenet],
            confidence: DiscoveryConfidence::Medium,
            source_id: Some(entry.code.clone()),
            already_in_library: false,
            warnings: Vec::new(),
        });
    }

    SourceScanResult {
        source: DiscoverySource::Battlenet,
        state: SourceState::Available,
        candidates,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：手工编码 protobuf 字段（合成 fixture 用）。
    fn encode_field(field: u32, bytes: &[u8]) -> Vec<u8> {
        let tag = (field << 3) | 2;
        let mut out = Vec::new();
        let mut tag_value = tag;
        while tag_value >= 0x80 {
            out.push((tag_value & 0x7F) as u8 | 0x80);
            tag_value >>= 7;
        }
        out.push(tag_value as u8);
        let mut len = bytes.len() as u64;
        while len >= 0x80 {
            out.push((len & 0x7F) as u8 | 0x80);
            len >>= 7;
        }
        out.push(len as u8);
        out.extend_from_slice(bytes);
        out
    }

    fn product_record(code: &str, path: Option<&str>) -> Vec<u8> {
        let mut record = encode_field(2, code.as_bytes());
        if let Some(path) = path {
            let mut sub = encode_field(1, path.as_bytes());
            sub.extend_from_slice(&encode_field(2, b"cn"));
            record.extend_from_slice(&encode_field(3, &sub));
        }
        record
    }

    /// 合成 fixture（标注：模拟数据，非真实 product.db）。
    fn synthetic_db() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&encode_field(
            1,
            &product_record("agent", Some("C:/Battle.net/Agent")),
        ));
        data.extend_from_slice(&encode_field(
            1,
            &product_record("bna", Some("D:/Battle.net")),
        ));
        data.extend_from_slice(&encode_field(
            1,
            &product_record("hsb", Some("D:/Hearthstone")),
        ));
        data.extend_from_slice(&encode_field(1, &product_record("wow", None)));
        data
    }

    #[test]
    fn parses_synthetic_db_and_excludes_launchers() {
        let (entries, skipped) = parse_product_db(&synthetic_db());
        // wow 记录没有安装路径 → 无法定位游戏，按设计计入 skipped
        assert_eq!(skipped, 1, "无路径的记录应计入 skipped");
        let codes: Vec<&str> = entries.iter().map(|entry| entry.code.as_str()).collect();
        assert_eq!(codes, vec!["agent", "bna", "hsb"]);
    }

    #[test]
    fn scan_skips_launchers_and_unknown_codes() {
        // wow 无已知 exe 路径匹配（目录不存在）→ skipped；hsb/agent/bna 按规则处理
        let dir = std::env::temp_dir().join("gamelib-bnet-scan-test");
        std::fs::create_dir_all(dir.join("Hearthstone")).expect("建目录");
        let exe = dir.join("Hearthstone").join("Hearthstone.exe");
        std::fs::write(&exe, b"stub").expect("写占位 exe");

        // 把合成记录里的路径指向临时目录
        let mut data = Vec::new();
        data.extend_from_slice(&encode_field(1, &product_record("agent", None)));
        data.extend_from_slice(&encode_field(1, &product_record("bna", None)));
        let hs_path = dir.join("Hearthstone").to_string_lossy().replace('\\', "/");
        data.extend_from_slice(&encode_field(1, &product_record("hsb", Some(&hs_path))));
        data.extend_from_slice(&encode_field(1, &product_record("unknown_code", None)));

        let db = dir.join("product.db");
        std::fs::write(&db, &data).expect("写 fixture");

        let result = scan_battlenet(Some(&db));
        assert!(matches!(result.state, SourceState::Available));
        assert_eq!(result.candidates.len(), 1, "只应产出炉石传说一条候选");
        assert_eq!(result.candidates[0].name, "炉石传说");
        assert!(
            result.candidates[0]
                .launch_path
                .replace('/', "\\")
                .eq_ignore_ascii_case(&exe.to_string_lossy().replace('/', "\\"))
        );
        // agent/bna 排除 + wow/unknown（无有效 exe）跳过
        assert!(result.skipped >= 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_data_is_reported_not_panic() {
        let (_, skipped) = parse_product_db(&[0xFF, 0xFF, 0xFF, 0xFF, 0x42]);
        assert!(skipped >= 1, "非法数据应计入 skipped 而非 panic");
        let (entries, _) = parse_product_db(&[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn truncated_record_does_not_drop_others() {
        // 完整 agent 记录 + 截断的 wow 记录：前者应保留
        let mut data = encode_field(1, &product_record("agent", Some("C:/Battle.net/Agent")));
        data.extend_from_slice(&encode_field(1, b"\x99"));
        let (entries, _skipped) = parse_product_db(&data);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].code, "agent");
    }

    #[test]
    fn missing_db_reports_not_installed() {
        let missing = std::env::temp_dir()
            .join("no-such-dir-9f3a2b")
            .join("product.db");
        let result = scan_battlenet(Some(&missing));
        assert!(
            matches!(result.state, SourceState::NotInstalled),
            "不存在的 product.db 应报告未安装，实际 {:?}",
            result.state
        );
    }
}
