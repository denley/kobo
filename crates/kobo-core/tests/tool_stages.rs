//! The build's tool stages: the project's Asar patches, early and late,
//! and AddmusicK. The patch stages need Asar's library (`KOBO_ASAR_LIB`)
//! and no ROM; the music stage needs AddmusicK (`KOBO_ADDMUSICK`) and the
//! vanilla ROM.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use kobo_core::SnesAddr;
use kobo_core::build::{self, Cache, Project};
use kobo_core::source::project::Manifest;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kobo-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, file: &str, text: &str) {
    let path = dir.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn patches_apply_early_and_late_in_order() {
    if common::asar().is_none() {
        return;
    }
    let base = common::synthetic_base();
    let dir = temp_dir("patches");
    write(&dir, "asm/shared.asm", "!value = $42\n");
    write(
        &dir,
        "asm/early.asm",
        "lorom\norg $0FF200\ndb $01, $02\nfreedata\nearly: db \"EARLY\"\norg $0FF210\ndl early\n",
    );
    write(
        &dir,
        "asm/late.asm",
        "lorom\nincsrc \"shared.asm\"\norg $0FF201\ndb !value\n",
    );
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            early_patches: vec![PathBuf::from("asm/early.asm")],
            late_patches: vec![PathBuf::from("asm/late.asm")],
            ..Manifest::default()
        },
        levels: Vec::new(),
    };
    let built = build::build_on(&base, &project, None).unwrap();
    // The late patch wrote over the early one's second byte.
    assert_eq!(
        built.read(SnesAddr::new(0x0FF200), 2).unwrap(),
        [0x01, 0x42]
    );
    let early = built.read_ptr(SnesAddr::new(0x0FF210)).unwrap();
    assert_eq!(built.read(early, 5).unwrap(), b"EARLY");
    assert!(built.internal_header().checksum_pair_valid());
    assert_eq!(
        build::build_on(&base, &project, None).unwrap().data(),
        built.data()
    );

    // An included file is an input: changing it changes the key.
    let cache = Cache::new(dir.join("cache"));
    build::build_on(&base, &project, Some(&cache)).unwrap();
    write(&dir, "asm/shared.asm", "!value = $43\n");
    let rebuilt = build::build_on(&base, &project, Some(&cache)).unwrap();
    assert_eq!(rebuilt.read_u8(SnesAddr::new(0x0FF201)).unwrap(), 0x43);

    // A failing patch names itself.
    write(
        &dir,
        "asm/late.asm",
        "lorom\norg $0FF201\nnot an instruction\n",
    );
    let error = build::build_on(&base, &project, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("late.asm"), "{error}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn addmusick_inserts_the_music() {
    let Some(tool) = std::env::var_os("KOBO_ADDMUSICK") else {
        eprintln!("skipping: KOBO_ADDMUSICK is not set");
        return;
    };
    assert!(Path::new(&tool).is_dir(), "KOBO_ADDMUSICK must be a folder");
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let dir = temp_dir("music");
    fs::create_dir_all(dir.join("music")).unwrap();
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            music: Some(PathBuf::from("music")),
            ..Manifest::default()
        },
        levels: Vec::new(),
    };
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(built.read(SnesAddr::new(0x0E8000), 4).unwrap(), b"@AMK");
    assert_eq!(built.len(), 0x10_0000);
    assert_eq!(build::build(&clean, &project).unwrap().data(), built.data());
    let _ = fs::remove_dir_all(&dir);
}
