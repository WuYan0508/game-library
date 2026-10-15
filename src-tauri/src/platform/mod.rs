//! Windows 平台专有调用。
//!
//! 本目录是**唯一**直接使用 `windows` crate 的地方 —— 集中封装便于日后替换，
//! 也让「无 COM」「无 STA」这类约束只需要在一处守住。

pub mod icon;
pub mod win;
