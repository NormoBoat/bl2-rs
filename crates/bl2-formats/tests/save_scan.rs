//! Read only COPIES of the user's saves. Inner WSG/profile semantics are not tested here (T-016).
use std::path::Path;

#[test]
#[ignore = "set BL2_SAVE_COPIES to a directory under .research/user-data containing copied saves"]
fn copied_save_envelopes() {
    fn scan(dir: &Path, count: &mut usize, total: &mut usize) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_symlink(), "save copies must not contain links");
            let path = std::fs::canonicalize(entry.path()).unwrap();
            assert!(path.starts_with(dir) && path != dir, "save copy escapes its directory");
            if path.is_dir() { scan(&path, count, total); continue; }
            let name = path.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            if name != "profile.bin" && !name.ends_with(".sav") && !name.ends_with(".sav.bak") { continue; }
            let bytes = std::fs::read(&path).unwrap();
            let payload = bl2_formats::save::decode_envelope(&bytes, 64 * 1024 * 1024).unwrap();
            if name != "profile.bin" {
                assert!(payload.len() >= 11);
                assert_eq!(u32::from_be_bytes(payload[..4].try_into().unwrap()) as usize, payload.len() - 4);
                assert_eq!(&payload[4..11], b"WSG\x02\0\0\0"); // observed in E-047; no interpretation past this point
            }
            *count += 1;
            *total += payload.len();
        }
    }
    let path = std::fs::canonicalize(std::env::var_os("BL2_SAVE_COPIES").expect("set BL2_SAVE_COPIES")).unwrap();
    let copies = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.research/user-data")).unwrap();
    assert!(path.starts_with(copies), "use copies under .research/user-data (D-015)");
    let (mut count, mut total) = (0, 0);
    scan(&path, &mut count, &mut total);
    assert!(count > 0, "no save copies found");
    println!("{count} save/profile envelopes verified, {total} decompressed bytes; inner payload not decoded");
}
