use std::fs;
use stellaris_docs_lib::testsupport::TempAppData;

#[test]
fn each_instance_is_an_isolated_writable_directory() {
    let first = TempAppData::new();
    let second = TempAppData::new();
    assert_ne!(first.path(), second.path());
    fs::write(first.path().join("state.json"), b"{}").unwrap();
    assert!(!second.path().join("state.json").exists());
}

#[test]
fn the_directory_is_removed_on_drop() {
    let path = {
        let data = TempAppData::new();
        assert!(data.path().is_dir());
        data.path().to_path_buf()
    };
    assert!(!path.exists());
}
