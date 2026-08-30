// #[derive(Deserialize)]
// pub(crate) struct BenchQuery {
//     size: Option<usize>,
//     iters: Option<usize>,
// }
//


// // 临时在测试各aead在wasm32的性能端点
// /**
// ❯ size=$((1024*1024))
// baseurl=http://127.0.0.1/aead/
//
// for alg in \
// chacha20poly1305 \
// ascon128
// do
// curl -s "$baseurl$alg?size=$size"
// echo " "
// done
// ================ [AEAD BENCHMARK] ==============
// Algorithm  : chacha20poly1305
// Payload    : 1.00 MB (1048576 bytes)
// Iterations : 50
// Total Data : 50.00 MB
// ------------------------------------------------
// [ Encryption ]
// Time       : 591 ms
// Throughput : 84.60 MB/s
// Speed      : 85 ops/sec
// ------------------------------------------------
// [ Decryption ]
// Time       : 650 ms
// Throughput : 76.92 MB/s
// Speed      : 77 ops/sec
// ================================================
// Anti-Opt(X): 19
// ================ [AEAD BENCHMARK] ==============
// Algorithm  : ascon128
// Payload    : 1.00 MB (1048576 bytes)
// Iterations : 50
// Total Data : 50.00 MB
// ------------------------------------------------
// [ Encryption ]
// Time       : 339 ms
// Throughput : 147.49 MB/s
// Speed      : 147 ops/sec
// ------------------------------------------------
// [ Decryption ]
// Time       : 298 ms
// Throughput : 167.79 MB/s
// Speed      : 168 ops/sec
// ================================================
// Anti-Opt(X): 252
// */
//
// #[worker::send]
// pub(crate) async fn bench_aead(
//     Path(aead_name): Path<String>,
//     Query(q): Query<BenchQuery>,
// ) -> Result<String, (StatusCode, String)> {
//     use std::str::FromStr;
//     use lib::algo::ProxyAead;
//
//
//
//     let aead = ProxyAead::from_str(aead_name.trim())
//         .map_err(|_| error!(BAD_REQUEST, "unsupported aead: {aead_name}"))?;
//
//     let size = q.size.unwrap_or(50 * 1024).min(1024 * 1024 * 50).max(1);
//     let iters = q.iters.unwrap_or(50).min(1024).max(1);
//
//     let key16 = *b"222222222ki9ks7b";
//     let key32 = *b"statw3e.key3xz3d3sdf57s.rke'ya32";
//
//     let payload: Vec<u8> = (0..size).map(|i| (i.wrapping_mul(7) & 0xff) as u8).collect();
//     let mut x = 0_u8;
//
//     // --- 加密测试 ---
//     let start_ms_enc = Date::now().as_millis();
//     let mut ciphertext: Vec<u8> = Vec::new();
//     for _ in 0..iters {
//         for &byte in ciphertext.iter().take(8) { x ^= byte; }
//         ciphertext = aead.encrypt(&payload, &key16, &key32)
//             .map_err(|e| error!(INTERNAL_SERVER_ERROR, "Encrypt failed: {}", e))?;
//     }
//     let end_ms_enc = Date::now().as_millis();
//
//     // --- 解密测试 ---
//     // sync_clock().await;
//     let start_ms_dec = Date::now().as_millis();
//     let mut plaintext: Vec<u8> = Vec::new();
//     for _ in 0..iters {
//         for &byte in plaintext.iter().take(8) { x ^= byte; }
//         plaintext = aead.decrypt(&ciphertext, &key16, &key32)
//             .map_err(|e| error!(INTERNAL_SERVER_ERROR, "Decrypt failed: {}", e))?;
//     }
//     let end_ms_dec = Date::now().as_millis();
//
//     drop(ciphertext);
//     drop(plaintext);
//
//     // --- 数据计算 ---
//     let elapsed_ms_enc = end_ms_enc.saturating_sub(start_ms_enc).max(1) as f64;
//     let elapsed_ms_dec = end_ms_dec.saturating_sub(start_ms_dec).max(1) as f64;
//
//     let total_bytes = (size as f64) * (iters as f64);
//     let total_mb = total_bytes / 1_048_576.0; // 1024 * 1024
//
//     // 计算吞吐量 (MB/s) = 总 MB / 总秒数
//     let enc_throughput = total_mb / (elapsed_ms_enc / 1000.0);
//     let dec_throughput = total_mb / (elapsed_ms_dec / 1000.0);
//
//     // 计算每秒操作数 (Ops/sec)
//     let enc_ops = (iters as f64) / (elapsed_ms_enc / 1000.0);
//     let dec_ops = (iters as f64) / (elapsed_ms_dec / 1000.0);
//
//     // 友好的 Payload 大小显示
//     let payload_display = if size >= 1048576 {
//         format!("{:.2} MB", size as f64 / 1048576.0)
//     } else if size >= 1024 {
//         format!("{:.2} KB", size as f64 / 1024.0)
//     } else {
//         format!("{} B", size)
//     };
//
//     // --- 生成报告 ---
//     let report = format!(
//         "================ [AEAD BENCHMARK] ==============\n\
//          Algorithm  : {aead_name}\n\
//          Payload    : {payload_display} ({size} bytes)\n\
//          Iterations : {iters}\n\
//          Total Data : {total_mb:.2} MB\n\
//          ------------------------------------------------\n\
//          [ Encryption ]\n\
//          Time       : {elapsed_ms_enc} ms\n\
//          Throughput : {enc_throughput:.2} MB/s\n\
//          Speed      : {enc_ops:.0} ops/sec\n\
//          ------------------------------------------------\n\
//          [ Decryption ]\n\
//          Time       : {elapsed_ms_dec} ms\n\
//          Throughput : {dec_throughput:.2} MB/s\n\
//          Speed      : {dec_ops:.0} ops/sec\n\
//          ================================================\n\
//          Anti-Opt(X): {x}",
//     );
//     lib::warn!("{}", report);
//
//     Ok(report)
// }
