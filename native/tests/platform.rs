use otools_releasehub_native::core::{
    models::ReleaseAsset,
    selection::{choose, classify_for_platform},
};
fn asset(name: &str) -> ReleaseAsset {
    ReleaseAsset {
        id: 1,
        name: name.into(),
        size: 1,
        url: format!("https://github.com/owner/app/releases/download/v1/{name}"),
        content_type: "application/octet-stream".into(),
        digest: None,
        kind: String::new(),
        compatible: false,
        score: 0,
        reason: String::new(),
    }
}
#[test]
fn macos_arm64_and_linux_x64_recommend_real_target_and_reject_windows() {
    for (os, arch, good, bad) in [
        (
            "macos",
            "aarch64",
            "app-macos-arm64.dmg",
            "app-windows-x64.exe",
        ),
        (
            "linux",
            "x86_64",
            "app-linux-amd64.AppImage",
            "app-macos-arm64.zip",
        ),
    ] {
        let mut compatible = asset(good);
        classify_for_platform(&mut compatible, "auto", None, os, arch);
        assert!(compatible.compatible, "{}", compatible.reason);
        assert_eq!(choose(&[compatible]), Some(1));
        let mut wrong = asset(bad);
        classify_for_platform(&mut wrong, "auto", None, os, arch);
        assert!(!wrong.compatible);
    }
}
#[test]
fn wrong_architecture_and_source_checksums_are_never_candidates() {
    for name in [
        "app-macos-x64.zip",
        "app-macos-arm64.zip.sha256",
        "app-source.zip",
    ] {
        let mut value = asset(name);
        classify_for_platform(&mut value, "portable", None, "macos", "aarch64");
        assert!(!value.compatible, "{name}");
    }
}
