// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
//! 凭据加密模块
//!
//! 主密钥通过系统钥匙串（macOS Keychain）存储，首次运行时自动生成。
//! 帐密与 cookie 使用 AES-256-GCM 加密后存入 SQLite。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{Context, Result};
use rand::RngCore;

const KEYRING_SERVICE: &str = "com.mutantcat.marketingconnect";
const KEYRING_USER: &str = "master-key";

/// 获取或创建主密钥（32 字节，存于系统钥匙串）
fn get_or_create_master_key() -> Result<[u8; 32]> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .context("创建钥匙串条目失败")?;

    // 尝试读取已有密钥
    if let Ok(hex_key) = entry.get_password() {
        if let Ok(bytes) = hex::decode(&hex_key) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }
    }

    // 生成新密钥
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    entry
        .set_password(&hex::encode(key))
        .context("写入钥匙串失败")?;
    Ok(key)
}

/// 加密明文，返回 nonce(12B) + ciphertext
pub fn encrypt(plaintext: &[u8]) -> Result<Vec<u8>> {
    let key_bytes = get_or_create_master_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("加密失败: {}", e))?;

    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// 解密 encrypt() 产生的数据
pub fn decrypt(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 12 {
        anyhow::bail!("密文数据过短");
    }
    let key_bytes = get_or_create_master_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Nonce::from_slice(&data[..12]);
    let plaintext = cipher
        .decrypt(nonce, &data[12..])
        .map_err(|e| anyhow::anyhow!("解密失败: {}", e))?;
    Ok(plaintext)
}

/// 加密字符串（UTF-8）
pub fn encrypt_str(s: &str) -> Result<Vec<u8>> {
    encrypt(s.as_bytes())
}

/// 解密为字符串（UTF-8）
pub fn decrypt_str(data: &[u8]) -> Result<String> {
    let bytes = decrypt(data)?;
    String::from_utf8(bytes).context("解密结果不是合法 UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        // 注意：此测试需要可用的系统钥匙串，在 CI 环境可能跳过
        let plaintext = "测试密码123456";
        let encrypted = encrypt_str(plaintext).unwrap();
        assert_ne!(encrypted, plaintext.as_bytes());
        let decrypted = decrypt_str(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
