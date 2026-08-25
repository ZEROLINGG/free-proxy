// lib/src/ws.rs
use std::collections::VecDeque;
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::{Serialize, Deserialize};

use crate::base::{Base64,Encoder};
use crate::hash::{Sha1,Hasher};



const MAX_WS_PAYLOAD_LEN: u64 = 64 * 1024 * 1024; // 64 MiB

/// WebSocket 操作码
#[repr(u8)]
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WsOpCode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Reserved3 = 0x3,
    Reserved4 = 0x4,
    Reserved5 = 0x5,
    Reserved6 = 0x6,
    Reserved7 = 0x7,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
    ReservedB = 0xB,
    ReservedC = 0xC,
    ReservedD = 0xD,
    ReservedE = 0xE,
    ReservedF = 0xF,
}

impl WsOpCode {
    pub fn is_control(&self) -> bool {
        *self as u8 >= 0x8
    }

    pub fn is_data(&self) -> bool {
        matches!(self, WsOpCode::Continuation | WsOpCode::Text | WsOpCode::Binary)
    }

    /// 是否为本实现不支持的保留操作码
    pub fn is_reserved(&self) -> bool {
        matches!(
            self,
            WsOpCode::Reserved3
                | WsOpCode::Reserved4
                | WsOpCode::Reserved5
                | WsOpCode::Reserved6
                | WsOpCode::Reserved7
                | WsOpCode::ReservedB
                | WsOpCode::ReservedC
                | WsOpCode::ReservedD
                | WsOpCode::ReservedE
                | WsOpCode::ReservedF
        )
    }
}

impl TryFrom<u8> for WsOpCode {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0x0 => Ok(WsOpCode::Continuation),
            0x1 => Ok(WsOpCode::Text),
            0x2 => Ok(WsOpCode::Binary),
            0x3 => Ok(WsOpCode::Reserved3),
            0x4 => Ok(WsOpCode::Reserved4),
            0x5 => Ok(WsOpCode::Reserved5),
            0x6 => Ok(WsOpCode::Reserved6),
            0x7 => Ok(WsOpCode::Reserved7),
            0x8 => Ok(WsOpCode::Close),
            0x9 => Ok(WsOpCode::Ping),
            0xA => Ok(WsOpCode::Pong),
            0xB => Ok(WsOpCode::ReservedB),
            0xC => Ok(WsOpCode::ReservedC),
            0xD => Ok(WsOpCode::ReservedD),
            0xE => Ok(WsOpCode::ReservedE),
            0xF => Ok(WsOpCode::ReservedF),
            _ => bail!("未知的 WebSocket WsOpCode: {:#04X}", value),
        }
    }
}

/// WebSocket 帧
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WsFrame {
    pub fin: bool,
    pub rsv1: bool,
    pub rsv2: bool,
    pub rsv3: bool,
    pub opcode: WsOpCode,
    pub mask_key: Option<[u8; 4]>,
    pub payload: Vec<u8>,
}

impl WsFrame {
    // ---------- 掩码相关 ----------


    /// 掩码/解掩码：XOR 是对合运算，加解掩码用同一函数
    fn apply_mask(payload: &mut [u8], mask_key: [u8; 4]) {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask_key[i & 3];
        }
    }

    // ---------- 便捷构造函数 ----------

    pub fn new_text(text: impl Into<String>, mask: Option<[u8; 4]>) -> Self {
        Self {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: WsOpCode::Text,
            mask_key: mask,
            payload: text.into().into_bytes(),
        }
    }

    pub fn new_binary(payload: Vec<u8>, mask: Option<[u8; 4]>) -> Self {
        Self {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: WsOpCode::Binary,
            mask_key: mask,
            payload,
        }
    }

    pub fn new_ping(payload: Vec<u8>, mask: Option<[u8; 4]>) -> Self {
        Self {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: WsOpCode::Ping,
            mask_key: mask,
            payload,
        }
    }

    pub fn new_pong(payload: Vec<u8>, mask: Option<[u8; 4]>) -> Self {
        Self {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: WsOpCode::Pong,
            mask_key: mask,
            payload,
        }
    }

    /// 构造 Close 帧
    pub fn new_close(data: Option<(u16, Option<String>)>, mask: Option<[u8; 4]>) -> Self {
        let mut payload = Vec::new();
        if let Some((code, reason_op)) = data {
            payload.extend_from_slice(&code.to_be_bytes());
            if let Some(reason) = reason_op {
                payload.extend_from_slice(reason.as_bytes());
            }
        }
        Self {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode: WsOpCode::Close,
            mask_key: mask,
            payload,
        }
    }

    // ---------- 语义判断/取值 ----------

    pub fn is_control(&self) -> bool {
        self.opcode.is_control()
    }

    pub fn is_data(&self) -> bool {
        self.opcode.is_data()
    }

    pub fn is_text(&self) -> bool {
        self.opcode == WsOpCode::Text
    }

    pub fn is_binary(&self) -> bool {
        self.opcode == WsOpCode::Binary
    }

    pub fn is_close(&self) -> bool {
        self.opcode == WsOpCode::Close
    }

    pub fn is_ping(&self) -> bool {
        self.opcode == WsOpCode::Ping
    }

    pub fn is_pong(&self) -> bool {
        self.opcode == WsOpCode::Pong
    }

    /// 从 Close 帧中提取状态码和原因（如果存在）
    pub fn close_info(&self) -> Option<(u16, Option<String>)> {
        if self.opcode != WsOpCode::Close || self.payload.is_empty() {
            return None;
        }
        if self.payload.len() >= 2 {
            let code = u16::from_be_bytes([self.payload[0], self.payload[1]]);

            let reason = if self.payload.len() > 2 {
                String::from_utf8(self.payload[2..].to_vec()).ok()
            } else {
                None
            };

            Some((code, reason))
        } else {
            None
        }
    }

    // ---------- 协议合法性校验 ----------

    /// 校验单帧是否符合 RFC 6455（不含分片重组、角色掩码等上层规则）
    pub fn validate(&self) -> Result<()> {
        // 未协商扩展的情况下 RSV 位必须为 0
        ensure!(
            !self.rsv1 && !self.rsv2 && !self.rsv3,
            "RSV 位必须为 0（未协商扩展）"
        );

        ensure!(
            !self.opcode.is_reserved(),
            "不支持的保留 WsOpCode: {:?}",
            self.opcode
        );

        if self.opcode.is_control() {
            ensure!(self.fin, "控制帧不允许分片 (FIN 必须为 1)");
            ensure!(self.payload.len() <= 125, "控制帧 payload 长度不能超过 125 字节");
        }

        ensure!(
            (self.payload.len() as u64) <= MAX_WS_PAYLOAD_LEN,
            "payload 长度超过最大限制 {} 字节",
            MAX_WS_PAYLOAD_LEN
        );

        if self.opcode == WsOpCode::Close && !self.payload.is_empty() {
            ensure!(self.payload.len() >= 2, "Close 帧 payload 长度必须为 0 或 >=2");
            if self.payload.len() > 2 {
                std::str::from_utf8(&self.payload[2..])
                    .context("Close 帧的 reason 字段不是合法的 UTF-8")?;
            }
        }

        Ok(())
    }

    // ---------- 序列化 ----------

    /// 序列化为可发送的字节流；消费 self 以便原地掩码，避免额外拷贝
    pub fn to_bytes(mut self) -> Vec<u8> {
        let payload_len = self.payload.len();
        let mut buf = Vec::with_capacity(payload_len + 14);

        // 第一字节：FIN + RSV1-3 + OPCODE
        let mut b0 = self.opcode as u8;
        if self.fin {
            b0 |= 0x80;
        }
        if self.rsv1 {
            b0 |= 0x40;
        }
        if self.rsv2 {
            b0 |= 0x20;
        }
        if self.rsv3 {
            b0 |= 0x10;
        }
        buf.push(b0);

        // 第二字节：MASK + payload len（含扩展长度）
        let mask_bit = if self.mask_key.is_some() { 0x80 } else { 0x00 };
        if payload_len <= 125 {
            buf.push(mask_bit | payload_len as u8);
        } else if payload_len <= 0xFFFF {
            buf.push(mask_bit | 126);
            buf.extend_from_slice(&(payload_len as u16).to_be_bytes());
        } else {
            buf.push(mask_bit | 127);
            buf.extend_from_slice(&(payload_len as u64).to_be_bytes());
        }

        // 掩码 key + payload（原地掩码）
        if let Some(mask_key) = self.mask_key {
            buf.extend_from_slice(&mask_key);
            Self::apply_mask(&mut self.payload, mask_key);
        }
        buf.extend_from_slice(&self.payload);

        buf
    }

    // ---------- 解析 ----------

    /// 尝试从 buf 中解析出一个完整帧。
    ///
    /// - `Ok(None)`：数据不完整，需要更多字节（不是错误）
    /// - `Ok(Some((frame, consumed)))`：解析成功，`consumed` 为消耗的字节数，
    ///   调用方应从 buf 中丢弃这部分数据
    /// - `Err`：协议违规（非法 opcode、长度非法、超过最大长度等）
    pub fn parse(buf: &[u8]) -> Result<Option<(Self, usize)>> {
        if buf.len() < 2 {
            return Ok(None);
        }

        let b0 = buf[0];
        let b1 = buf[1];

        let fin = b0 & 0x80 != 0;
        let rsv1 = b0 & 0x40 != 0;
        let rsv2 = b0 & 0x20 != 0;
        let rsv3 = b0 & 0x10 != 0;
        let opcode = WsOpCode::try_from(b0 & 0x0F)?;

        let masked = b1 & 0x80 != 0;
        let len_field = b1 & 0x7F;

        let mut offset = 2usize;

        let payload_len: u64 = match len_field {
            126 => {
                if buf.len() < offset + 2 {
                    return Ok(None);
                }
                let len =
                    u16::from_be_bytes(buf[offset..offset + 2].try_into().unwrap()) as u64;
                offset += 2;
                len
            }
            127 => {
                if buf.len() < offset + 8 {
                    return Ok(None);
                }
                let len = u64::from_be_bytes(buf[offset..offset + 8].try_into().unwrap());
                offset += 8;
                ensure!(
                    len & 0x8000_0000_0000_0000 == 0,
                    "非法的 payload 长度：64 位长度字段最高位必须为 0"
                );
                len
            }
            n => n as u64,
        };

        ensure!(
            payload_len <= MAX_WS_PAYLOAD_LEN,
            "payload 长度 {} 超过最大限制 {} 字节",
            payload_len,
            MAX_WS_PAYLOAD_LEN
        );

        let mask_key = if masked {
            if buf.len() < offset + 4 {
                return Ok(None);
            }
            let key: [u8; 4] = buf[offset..offset + 4].try_into().unwrap();
            offset += 4;
            Some(key)
        } else {
            None
        };

        let total_len = offset + payload_len as usize;
        if buf.len() < total_len {
            return Ok(None);
        }

        let mut payload = buf[offset..total_len].to_vec();
        if let Some(key) = mask_key {
            Self::apply_mask(&mut payload, key);
        }

        let frame = WsFrame {
            fin,
            rsv1,
            rsv2,
            rsv3,
            opcode,
            mask_key,
            payload,
        };

        // 语法解析完成后做一次语义校验，尽早暴露非法帧
        frame.validate()?;

        Ok(Some((frame, total_len)))
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsData {
    Text(String),
    Binary(Vec<u8>),
}

/// 用于 Text，Binary 的分片重组处理
pub struct WsCache {
    cache: VecDeque<WsFrame>,
    current_cache_size: u64,

}

impl WsCache {
    pub fn new() -> WsCache {
        WsCache {
            cache: VecDeque::new(),
            current_cache_size: 0,
        }
    }

    pub fn push(&mut self, frame: WsFrame) -> Result<()> {
        if !frame.is_data() {
            return Ok(());
        }

        if self.cache.is_empty() {
            ensure!(
                frame.opcode == WsOpCode::Text || frame.opcode == WsOpCode::Binary,
                "协议违规: 收到无头部的 Continuation 数据帧"
            );
        } else {
            ensure!(
                frame.opcode == WsOpCode::Continuation,
                "协议违规: 分片消息未结束时，收到了非 Continuation 的数据帧: {:?}",
                frame.opcode
            );
        }

        self.current_cache_size += frame.payload.len() as u64;
        ensure!(
            self.current_cache_size <= MAX_WS_PAYLOAD_LEN * 2,
            "重组消息总长度超过最大限制 {} 字节",
            MAX_WS_PAYLOAD_LEN * 2
        );

        self.cache.push_back(frame);
        Ok(())
    }

    pub fn try_pop(&mut self) -> Result<Option<WsData>> {
        if self.cache.is_empty() {
            return Ok(None);
        }

        let is_complete = self.cache.back().unwrap().fin;
        if !is_complete {
            return Ok(None); // 数据还不完整，等待下一个 frame
        }


        let mut first_frame = self.cache.pop_front().unwrap();
        let first_opcode = first_frame.opcode;
        let mut full_payload = std::mem::take(&mut first_frame.payload);

        let total_payload_len = self.current_cache_size as usize;
        full_payload.reserve(total_payload_len - full_payload.len());

        for next_frame in self.cache.drain(..) {
            full_payload.extend_from_slice(&next_frame.payload);
        }

        self.current_cache_size = 0;

        match first_opcode {
            WsOpCode::Text => {
                let text = String::from_utf8(full_payload)
                    .context("重组后的 Text 消息包含非法的 UTF-8 字符")?;
                Ok(Some(WsData::Text(text)))
            }
            WsOpCode::Binary => Ok(Some(WsData::Binary(full_payload))),
            _ => unreachable!(),
        }
    }
}




pub fn calc_sec_ws_accept(ws_key: &str) -> String {
    const MAGIC_GUID: &'static str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let concat_str = format!("{}{}", ws_key, MAGIC_GUID);
    let sha1_digest = Sha1::digest_vec(concat_str);
    Base64::encode(sha1_digest).expect("calc_sec_ws_accept Base64 编码失败")
}

// Worker代理ws时中间转发的数据
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum WsTunnelMsg {
    HeadFrame(Vec<u8>), // 原始ws升级请求
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<(u16,Option<String>)>),
    Return(Vec<u8>),     // 下行的可直接写入tcp的数据
    Error(String)
}

impl WsTunnelMsg {
    pub fn serialize(&self) -> Result<Vec<u8>> {
        postcard::to_allocvec(self)
            .map_err(|e| anyhow!("WsTunnelMsg serialize failed: {e}"))
    }
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        postcard::from_bytes(data)
            .map_err(|e| anyhow!("WsTunnelMsg deserialize failed: {e}"))
    }
}
