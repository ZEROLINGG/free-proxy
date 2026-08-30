// lib_test/src/util.rs
//
// 下载/上传测试共用的确定性数据生成与完整性校验工具。
// 生成端（web.rs 路由）与校验端（test/http.rs）必须使用同一套函数，
// 因此集中放在本模块，杜绝两端实现漂移。

use anyhow::Result;

/// 位置派生字节：给定流内偏移 pos 返回确定的字节值。
/// 故意选用带位混淆的整数散列，使输出近似随机（不可压缩、
/// 含任意字节序列），同时保持 O(1) 空间与完全确定性。
#[inline]
pub fn gen_byte(pos: u64) -> u8 {
    let x = pos.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let y = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (y ^ (y >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) & 0xFF) as u8
}

/// 按位置派生算法填充 buf[0..]（pos 从 base 开始计）
pub fn fill_pattern(buf: &mut [u8], base: u64) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = gen_byte(base + i as u64);
    }
}

/// 生成长度为 size 的模式数据
pub fn pattern_bytes(size: usize) -> Vec<u8> {
    let mut v = vec![0u8; size];
    fill_pattern(&mut v, 0);
    v
}


/// 校验「长度 + blake3」回显串（web.rs /upload 的返回格式）：
/// 格式 `len=<n>;blake3=<hex>`
pub fn parse_len_blake3(text: &str) -> Result<(u64, String)> {
    let text = text.trim();
    let (l, h) = text
        .split_once(';')
        .ok_or_else(|| anyhow::anyhow!("bad echo format: {text:?}"))?;
    let len: u64 = l
        .strip_prefix("len=")
        .ok_or_else(|| anyhow::anyhow!("missing len=: {l:?}"))?
        .parse()?;
    let hash = h
        .strip_prefix("blake3=")
        .ok_or_else(|| anyhow::anyhow!("missing blake3=: {h:?}"))?
        .to_string();
    Ok((len, hash))
}
