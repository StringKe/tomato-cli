//! 番茄网页端 `/api/*` 的 a_bogus 签名，移植自 Evil0ctal/Douyin_TikTok_Download_API 的 `abogus.py`（Apache-2.0，clean-room 逆向 bdms.js v1.0.1.19）。
//!
//! 服务端只校验三条摘要（query、body、User-Agent 各 3 字节）和内部 XOR 校验和，时间戳、cookie、msToken 不参与，签名可重放。
//! 所以 query 必须是最终发出的原始串（顺序、百分号大小写逐字节一致），User-Agent 必须是请求头里的同一个字符串。
//! 随机字节只影响外观，不影响服务端判定；其中三个「噪声」位其实是 SDK 的环境报告，照蓝本填浏览器家族和 tripwire 健康值，不填均匀随机。

use std::time::{SystemTime, UNIX_EPOCH};

use sm3::{Digest, Sm3};

/// s3 编码进入 UA 摘要的密文，s4 编码最终签名。
const S3: &[u8; 64] = b"ckdp1h4ZKsUB80/Mfvw36XIgR25+WQAlEi7NLboqYTOPuzmFjJnryx9HVGDaStCe";
const S4: &[u8; 64] = b"Dkdpgh2ZmsQB80/MfvV36XI1R45-WUAlEixNLwoqYTOPuzKFjJnry79HbGcaStCe";
const SALT: &str = "dhzx";
const HEADER_MAGIC: [u8; 2] = [3, 82];
const SDK_VERSION: [u8; 4] = [1, 0, 1, 0];
const PAYLOAD_KEY: [u8; 1] = [0xD3];
/// 2024-07-24T16:00:00Z，L26 记录从这里起算的双周数。
const FORTNIGHT_EPOCH_MS: u64 = 1_721_836_800_000;
const FORTNIGHT_MS: u64 = 1000 * 60 * 60 * 24 * 14;
/// 番茄网页端配置给 bdms 的页面标识和 aid，与请求 query 无关，只进签名。
const PAGE_ID: u32 = 2503;
const AID: u32 = 24117;
/// 三个掩码互补拼成 0xFF，第四个载体字节正好收下前三个字节让给噪声的位，层才可逆。
const NOISE_MASKS: [u8; 3] = [0x91, 0x42, 0x2C];
const DATA_MASKS: [u8; 3] = [0x6E, 0xBD, 0xD3];
/// 五十个标量按此顺序进入 body，是固定置换，按字节码原样抄。
const FIELD_ORDER: [usize; 50] = [34, 44, 56, 61, 73, 29, 70, 45, 35, 49, 38, 66, 51, 68, 28, 48, 64, 47, 30, 71, 26, 55, 31, 69, 59, 40, 62, 63, 27, 72, 41, 74, 57, 52, 42, 39, 33, 67, 53, 43, 65, 46, 36, 24, 60, 32, 79, 80, 84, 85];
/// 未被篡改的浏览器上六个探针和 bot 检测位集的取值。
const ENV_FLAGS: u16 = 1;
const DETECT_FLAGS: u32 = 14;
const NR_FLAGS: u16 = 0x21;
const NR_TAG: [u8; 4] = [0, 0, 0, 0];
/// `window.onwheelx._Ax` 存在且已冻结；12 表示被解锁，11 表示缺失，都会被打分。
const TRIPWIRE_LOCKED: u8 = 3;
/// 页面签名次数少于 140 的桶。
const CALL_BUCKET: u8 = 6;
/// 服务端不校验几何串内容，但每次变化的几何本身是特征，所以用固定常量而不是真实终端尺寸。
const DEFAULT_BROWSER_INFO: &str = "1920|947|1920|1032|1920|1032|1920|1080|Win32";
/// 摘要 canary：从 offset 起第一个不等于 sentinel 的字节，全等于时回退 fallback。
const CANARIES: [(usize, u8, u8); 3] = [(3, 11, 12), (4, 8, 9), (5, 12, 13)];
/// tripwire 报告的健康位：bit 1、4、5、7 置位表示三个 tripwire 都在且已冻结。
const TRIPWIRE_SET: u8 = 0xB2;
const TRIPWIRE_FREE: u8 = 0x4D;

/// 计算 `a_bogus`。`query` 是不含 `a_bogus` 本身、将要原样发出的 query 串；`user_agent` 是同一请求的 User-Agent 头。
pub fn a_bogus(query: &str, user_agent: &str) -> String {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(FORTNIGHT_EPOCH_MS);
    let mut rng = XorShift::from_clock();
    sign(query, "", user_agent, now_ms, &mut || rng.random())
}

/// `rng` 每次返回 [0, 1) 的浮点数，对应 SDK 的 `Math.random()`；抽出来是为了让测试能固定噪声与 Python 蓝本比对。
fn sign(query: &str, body: &str, user_agent: &str, now_ms: u64, rng: &mut dyn FnMut() -> f64) -> String {
    let now_ms = now_ms.max(FORTNIGHT_EPOCH_MS);
    let fields = fields(query, body, user_agent, now_ms);

    let mut version = [0u8; 8];
    version[..4].copy_from_slice(&mask_pair([SDK_VERSION[0], SDK_VERSION[1]], rng, None, None));
    let probe = probe_noise(rng);
    let tripwire = tripwire_noise(rng);
    version[4..].copy_from_slice(&mask_pair([SDK_VERSION[2], SDK_VERSION[3]], rng, Some(probe), Some(tripwire)));

    let mut checksum = version.iter().fold(0u8, |acc, b| acc ^ b);
    let mut body_bytes: Vec<u8> = FIELD_ORDER.iter().map(|&i| fields[i]).collect();
    for b in &body_bytes {
        checksum ^= b;
    }
    body_bytes.extend(js_bytes(DEFAULT_BROWSER_INFO));
    body_bytes.extend(js_bytes(&tail_text(now_ms)));
    body_bytes.push(checksum);

    let header_high = header_noise(user_agent, rng);
    let header = mask_pair(HEADER_MAGIC, rng, None, Some(header_high));
    let frame = expand_noise(&body_bytes, rng);
    let mut plain = version.to_vec();
    plain.extend(frame);
    let sealed = rc4(&PAYLOAD_KEY, &plain);
    let mut payload = header.to_vec();
    payload.extend(sealed);
    encode_base64(&payload, S4)
}

/// 五十个标量按 L 编号存放，下标即编号，24 以下不用。
fn fields(query: &str, body: &str, user_agent: &str, now_ms: u64) -> [u8; 86] {
    let mut f = [0u8; 86];
    f[24] = 41;
    f[26] = ((now_ms - FORTNIGHT_EPOCH_MS) / FORTNIGHT_MS) as u8;
    f[27] = CALL_BUCKET;
    // SDK 初始化到进入签名入口的毫秒数加 3；进程不常驻页面，「刚初始化就签」是诚实值。
    f[28] = 3;
    f[35] = ENV_FLAGS as u8;
    f[36] = (ENV_FLAGS >> 8) as u8;
    f[38] = NR_FLAGS as u8;
    f[39] = (NR_FLAGS >> 8) as u8;
    f[66] = TRIPWIRE_LOCKED;
    let info_len = js_bytes(DEFAULT_BROWSER_INFO).len();
    f[79] = info_len as u8;
    f[80] = (info_len >> 8) as u8;
    let tail_len = js_bytes(&tail_text(now_ms)).len();
    f[84] = tail_len as u8;
    f[85] = (tail_len >> 8) as u8;
    for (i, b) in le_bytes(now_ms, 6).into_iter().enumerate() {
        f[29 + i] = b;
    }
    for (i, b) in le_bytes(DETECT_FLAGS as u64, 4).into_iter().enumerate() {
        f[44 + i] = b;
    }
    for (i, b) in NR_TAG.into_iter().enumerate() {
        f[40 + i] = b;
    }
    // ink 是 SDK 前一步种在 navigator 原型上的 Date.now() - 1，服务端用它核对时钟自洽。
    for (i, b) in le_bytes(now_ms - 1, 6).into_iter().enumerate() {
        f[60 + i] = b;
    }
    for (i, b) in le_bytes(PAGE_ID as u64, 4).into_iter().enumerate() {
        f[67 + i] = b;
    }
    for (i, b) in le_bytes(AID as u64, 4).into_iter().enumerate() {
        f[71 + i] = b;
    }
    // 每条摘要只写 3 个字节：两个定点取值和一个 canary，这就是签名对 query / body / UA 的全部绑定。
    let chains = [
        DigestChain { slots: [48, 49, 51], indices: [9, 18], canary: CANARIES[0], digest: digest_of(query) },
        DigestChain { slots: [52, 53, 55], indices: [10, 19], canary: CANARIES[1], digest: digest_of(body) },
        DigestChain { slots: [56, 57, 59], indices: [11, 21], canary: CANARIES[2], digest: user_agent_digest(user_agent) },
    ];
    for chain in chains {
        let (offset, sentinel, fallback) = chain.canary;
        f[chain.slots[0]] = chain.digest[chain.indices[0]];
        f[chain.slots[1]] = chain.digest[chain.indices[1]];
        f[chain.slots[2]] = canary(&chain.digest, offset, sentinel, fallback);
    }
    f
}

/// 一条被哈希的输入在五十个标量里的落点：slots 是 L 编号，indices 是原样写入的两个摘要下标，canary 是第三个槽位遵循的 sentinel 三元组。
struct DigestChain {
    slots: [usize; 3],
    indices: [usize; 2],
    canary: (usize, u8, u8),
    digest: [u8; 32],
}

fn tail_text(now_ms: u64) -> String {
    format!("{},", (now_ms + 3) & 0xFF)
}

fn le_bytes(value: u64, count: usize) -> Vec<u8> {
    (0..count).map(|i| (value >> (8 * i)) as u8).collect()
}

fn canary(digest: &[u8; 32], offset: usize, sentinel: u8, fallback: u8) -> u8 {
    digest[offset..].iter().copied().find(|&b| b != sentinel).unwrap_or(fallback)
}

fn sm3(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sm3::digest(data));
    out
}

/// query 和 body 的摘要：`SM3(SM3(text + SALT))`。
fn digest_of(text: &str) -> [u8; 32] {
    let mut salted = text.as_bytes().to_vec();
    salted.extend_from_slice(SALT.as_bytes());
    sm3(&sm3(&salted))
}

/// UA 链只做一次 SM3，之前先用环境探针拼出的三字节密钥做 RC4 再 s3 编码。密文按 UTF-16 code unit 走，不按 UTF-8。
fn user_agent_digest(user_agent: &str) -> [u8; 32] {
    let key = [(ENV_FLAGS >> 8) as u8, ENV_FLAGS as u8, DETECT_FLAGS as u8];
    let sealed = rc4(&key, &js_bytes(user_agent.trim()));
    sm3(encode_base64(&sealed, S3).as_bytes())
}

/// SDK 的 RC4 变体：S 盒降序初始化，密钥调度里 j 乘上 box[i]。密钥流部分与标准 RC4 相同。
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut sbox = [0u8; 256];
    for (i, slot) in sbox.iter_mut().rev().enumerate() {
        *slot = i as u8;
    }
    let mut j = 0usize;
    for i in 0..256 {
        j = (j * sbox[i] as usize + j + key[i % key.len()] as usize) % 256;
        sbox.swap(i, j);
    }
    let (mut i, mut j) = (0usize, 0usize);
    data.iter()
        .map(|&byte| {
            i = (i + 1) % 256;
            j = (j + sbox[i] as usize) % 256;
            sbox.swap(i, j);
            byte ^ sbox[(sbox[i] as usize + sbox[j] as usize) % 256]
        })
        .collect()
}

/// 标准 base64 分组和 `=` 补位，只换字母表。
fn encode_base64(data: &[u8], table: &[u8; 64]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let mut block = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            block |= (*b as u32) << (16 - 8 * i);
        }
        for shift in [18u32, 12, 6, 0].iter().take(chunk.len() + 1) {
            out.push(table[((block >> shift) & 0x3F) as usize] as char);
        }
    }
    while !out.len().is_multiple_of(4) {
        out.push('=');
    }
    out
}

/// JS 字符串转字节的 SDK 规则：按 UTF-16 code unit，U+0100 以下一字节，以上高低两字节，不是 UTF-8。
fn js_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for code in text.encode_utf16() {
        if code & 0xFF00 != 0 {
            out.push((code >> 8) as u8);
        }
        out.push(code as u8);
    }
    out
}

/// 两个字节摊到四个载体，每个载体一半位来自 payload、一半来自噪声。`low`/`high` 让调用方把环境报告放进噪声半区。
fn mask_pair(pair: [u8; 2], rng: &mut dyn FnMut() -> f64, low: Option<u8>, high: Option<u8>) -> [u8; 4] {
    let noise = (rng() * 65535.0) as u32;
    let low = low.unwrap_or((noise & 0xFF) as u8);
    let high = high.unwrap_or(((noise >> 8) & 0xFF) as u8);
    [(low & 0xAA) | (pair[0] & 0x55), (low & 0x55) | (pair[0] & 0xAA), (high & 0xAA) | (pair[1] & 0x55), (high & 0x55) | (pair[1] & 0xAA)]
}

/// 三个 body 字节加一个噪声字节变四个载体字节；不足三个的尾组按 SDK 原样直出，第二个字节为 0 时丢弃。
fn expand_noise(body: &[u8], rng: &mut dyn FnMut() -> f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() / 3 * 4 + 2);
    for group in body.chunks(3) {
        if group.len() < 3 {
            out.push(group[0]);
            if group.len() > 1 && group[1] != 0 {
                out.push(group[1]);
            }
            continue;
        }
        let noise = ((rng() * 1000.0) as u32 & 0xFF) as u8;
        for k in 0..3 {
            out.push((noise & NOISE_MASKS[k]) | (group[k] & DATA_MASKS[k]));
        }
        out.push((group[0] & NOISE_MASKS[0]) | (group[1] & NOISE_MASKS[1]) | (group[2] & NOISE_MASKS[2]));
    }
    out
}

/// 头部第二个噪声字节是浏览器家族报告：每族 40 宽的区间，落在区间外或与 UA 不符都会被打分。
fn header_noise(user_agent: &str, rng: &mut dyn FnMut() -> f64) -> u8 {
    let ua = user_agent.to_ascii_lowercase();
    // Edge 和 Huawei 的 UA 也含 Chrome，Chrome 也含 Safari，所以先测具体名再测泛名。
    let base: u32 = if ua.contains("edg") {
        125
    } else if ua.contains("huawei") {
        170
    } else if ua.contains("firefox") {
        40
    } else if ua.contains("chrome") {
        0
    } else if ua.contains("safari") {
        81
    } else {
        210
    };
    ((base + (rng() * 40.0) as u32) & 0xFF) as u8
}

/// 探针噪声：110 以下原样通过，以上强制为奇数，使 110 到 240 之间一半取值不可达。
fn probe_noise(rng: &mut dyn FnMut() -> f64) -> u8 {
    let value = (rng() * 240.0) as u32;
    let value = if value > 109 { value + value % 2 + 1 } else { value };
    value as u8
}

fn tripwire_noise(rng: &mut dyn FnMut() -> f64) -> u8 {
    (((rng() * 255.0) as u32 as u8) & TRIPWIRE_FREE) | TRIPWIRE_SET
}

/// 噪声只影响签名外观，不需要密码学随机，用时钟做种的 xorshift 足够，省掉 rand 依赖。
struct XorShift(u64);

impl XorShift {
    fn from_clock() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self(nanos ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }

    fn random(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36";
    const QUERY: &str = "filter=127%2C127%2C127%2C127&page_count=10&page_index=0&query_type=0&query_word=%E5%8D%81%E6%97%A5%E7%BB%88%E7%84%89";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// 与 gen_vectors.py 里 FixedRng 相同的序列：第 n 次返回 ((n * 37) % 97) / 97。
    fn fixed_rng() -> impl FnMut() -> f64 {
        let mut n = 0u64;
        move || {
            let v = ((n * 37) % 97) as f64 / 97.0;
            n += 1;
            v
        }
    }

    #[test]
    fn sm3_known_vectors() {
        assert_eq!(hex(&sm3(b"abc")), "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0");
        assert_eq!(hex(&sm3(b"")), "1ab21d8355cfa17f8e61194831e81a8f22bec8c728fefb747ed035eb5082aa2b");
    }

    #[test]
    fn custom_base64_matches_blueprint_and_roundtrips() {
        assert_eq!(encode_base64(b"abcde", S3), "RIsN246=");
        assert_eq!(encode_base64(&[0, 1, 2, 3, 4, 5, 6], S4), "DDgdDjfhkE==");
        for table in [S3, S4] {
            for len in 0..10usize {
                let data: Vec<u8> = (0..len as u8).map(|i| i.wrapping_mul(53).wrapping_add(7)).collect();
                let text = encode_base64(&data, table);
                assert_eq!(text.len() % 4, 0);
                assert_eq!(decode_base64(&text, table), data);
            }
        }
    }

    #[test]
    fn rc4_variant_matches_python_blueprint() {
        let data: Vec<u8> = (0..16).collect();
        assert_eq!(hex(&rc4(&[0xD3], &data)), "7c2122643dac8faa2aa974dbce90cd72");
        assert_eq!(hex(&rc4(&[0, 1, 14], b"hello a_bogus")), "d8cfc71378ae9a2206401f56e1");
    }

    #[test]
    fn digest_chains_match_python_blueprint() {
        assert_eq!(hex(&digest_of(QUERY)), "16ef532eacd6f3b0317ea55e3db457f2cf9bb6b802b668cfe4d83c5e1aba6923");
        assert_eq!(hex(&digest_of("")), "40fd9cf02c609f961b7a5234c578ea77f55947b163621c8e05637bc7b00998f0");
        assert_eq!(hex(&user_agent_digest(UA)), "621e5750331479b95e400c2f94b19a4b643995f4a4760e838ca300930d94d725");
    }

    #[test]
    fn js_bytes_uses_utf16_units() {
        assert_eq!(js_bytes("aé"), vec![0x61, 0xE9]);
        assert_eq!(js_bytes("中"), vec![0x4E, 0x2D]);
    }

    #[test]
    fn full_signature_matches_python_blueprint_with_fixed_noise() {
        let mut rng = fixed_rng();
        let sig = sign(QUERY, "", UA, 1_789_000_000_123, &mut rng);
        assert_eq!(sig, "QJ4fhHyEm2WnCdFtmOJUeHdlq06/NBuyYzTxbN9cCxTELwFTJhprAQc6rowLsPxRRYQPg917sxUlbDVcp30hpAnkKmkDuxkRCt5A9hmohqqmGFkQLNj0ez6FKw0rU5GqeAVXiIU6hUrqgjnAwrQ8/pl9yKoe5RuBFpOSkMubi9s6ZzLAD3n3PQGkiwNzUU5f");
    }

    #[test]
    fn live_signature_has_expected_shape() {
        let sig = a_bogus(QUERY, UA);
        assert!((188..=192).contains(&sig.len()), "len {}", sig.len());
        assert!(sig.trim_end_matches('=').bytes().all(|b| S4.contains(&b)));
        let payload = decode_base64(&sig, S4);
        assert_eq!([(payload[0] & 0x55) | (payload[1] & 0xAA), (payload[2] & 0x55) | (payload[3] & 0xAA)], HEADER_MAGIC);
    }

    fn decode_base64(text: &str, table: &[u8; 64]) -> Vec<u8> {
        let body = text.trim_end_matches('=').as_bytes();
        let mut out = Vec::new();
        for chunk in body.chunks(4) {
            let mut block = 0u32;
            for c in chunk {
                block = (block << 6) | table.iter().position(|t| t == c).expect("digit") as u32;
            }
            block <<= 6 * (4 - chunk.len() as u32);
            for shift in [16u32, 8, 0].iter().take(chunk.len() - 1) {
                out.push(((block >> shift) & 0xFF) as u8);
            }
        }
        out
    }
}
