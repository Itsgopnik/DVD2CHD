use anyhow::Result;
use digest::Digest;
use md5::Md5;
use sha1::Sha1;
use sha2::Sha256;
use std::{
    fs,
    io::{BufReader, Read as _},
    path::Path,
    sync::Arc,
};

use crate::ProgressSink;

/// Compute MD5/SHA1/SHA-256 for a file.
pub fn compute_hashes(
    path: &Path,
    do_md5: bool,
    do_sha1: bool,
    do_sha256: bool,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    // Use a buffered reader to reduce the number of system calls when reading
    // the file. Using a `BufReader` is recommended for file hashing in Rust to
    // improve I/O performance.
    let f = fs::File::open(path)?;
    // Use a larger reader buffer to reduce the number of read syscalls for big files.
    let mut reader = BufReader::with_capacity(64 * 1024, f);
    let mut buf = vec![0u8; 128 * 1024];
    let mut md5_h = do_md5.then(Md5::new);
    let mut sha1_h = do_sha1.then(Sha1::new);
    let mut sha256_h = do_sha256.then(Sha256::new);
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if let Some(ref mut h) = md5_h {
            h.update(&buf[..n]);
        }
        if let Some(ref mut h) = sha1_h {
            h.update(&buf[..n]);
        }
        if let Some(ref mut h) = sha256_h {
            h.update(&buf[..n]);
        }
    }
    let md5_s = md5_h.map(|h| format!("{:x}", h.finalize()));
    let sha1_s = sha1_h.map(|h| format!("{:x}", h.finalize()));
    let sha256_s = sha256_h.map(|h| format!("{:x}", h.finalize()));
    Ok((md5_s, sha1_s, sha256_s))
}

/// Compute optional MD5/SHA1/SHA-256 hashes for a file and log the results to the provided
/// `ProgressSink`. This helper wraps [`compute_hashes`] to avoid repeating
/// logging logic in multiple call sites.
pub(crate) fn log_hashes(
    path: &Path,
    do_md5: bool,
    do_sha1: bool,
    do_sha256: bool,
    sink: &Arc<dyn ProgressSink>,
) -> Result<()> {
    let (md5, sha1, sha256) = compute_hashes(path, do_md5, do_sha1, do_sha256)?;
    if let Some(h) = md5 {
        sink.log(&format!("MD5   : {}", h));
    }
    if let Some(h) = sha1 {
        sink.log(&format!("SHA1  : {}", h));
    }
    if let Some(h) = sha256 {
        sink.log(&format!("SHA256: {}", h));
    }
    Ok(())
}
