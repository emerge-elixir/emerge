mod event_names;
mod sources;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn files(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("downloaded sources")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect();
    files.sort();
    files
}

fn patch(source: &Path, output: &Path, replacements: &[(&str, &str)]) {
    let mut text = fs::read_to_string(source).unwrap();
    for (old, new) in replacements {
        assert_eq!(
            text.matches(old).count(),
            1,
            "upstream patch no longer applies: {}: {old}",
            source.display()
        );
        text = text.replace(old, new);
    }
    fs::write(output, text).unwrap();
}

fn main() {
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "linux",
        "bundled libinput is Linux-only"
    );
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let cache = sources::cache_dir(
        &out,
        env::var_os("EMERGE_INPUT_CACHE_DIR").map(PathBuf::from),
    );
    let offline = ["EMERGE_INPUT_OFFLINE", "CARGO_NET_OFFLINE"]
        .iter()
        .any(|name| env::var(name).is_ok_and(|value| value == "1" || value == "true"));
    for name in [
        "EMERGE_INPUT_CACHE_DIR",
        "EMERGE_INPUT_OFFLINE",
        "CARGO_NET_OFFLINE",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let upstream: Vec<_> = sources::SOURCES
        .iter()
        .map(|source| {
            let directory = sources::unpack(source, &cache, &out, offline)
                .unwrap_or_else(|e| panic!("could not prepare {}: {e}", source.directory));
            println!(
                "cargo:rerun-if-changed={}",
                cache.join(source.archive).display()
            );
            directory
        })
        .collect();
    let [libinput, evdev, mtdev, udev] = upstream.as_slice() else {
        unreachable!("four pinned input sources");
    };
    let input = libinput.join("src");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=sources.rs");
    println!("cargo:rerun-if-changed=event_names.rs");
    println!("cargo:rerun-if-changed=shim.c");
    println!("cargo:rerun-if-changed=classification-test.c");
    println!("cargo:rerun-if-changed=quirks-bundle.h");
    fs::write(
        out.join("config.h"),
        r#"
#define _GNU_SOURCE 1
#define HAVE_MTDEV 1
#define HAVE_LOCALE_H 1
#define HAVE_VERSIONSORT 1
#define HTTP_DOC_LINK "https://wayland.freedesktop.org/libinput/doc/1.29.2"
#define LIBINPUT_QUIRKS_DIR "emerge:bundled"
#define LIBINPUT_QUIRKS_OVERRIDE_FILE NULL
#define MESON_BUILD_ROOT ""
#define PACKAGE_VERSION "1.13.4"
#define PACKAGE_NAME "libevdev"
"#,
    )
    .unwrap();
    fs::write(out.join("libinput-version.h"), "#define LIBINPUT_VERSION_MAJOR 1\n#define LIBINPUT_VERSION_MINOR 29\n#define LIBINPUT_VERSION_MICRO 2\n#define LIBINPUT_VERSION \"1.29.2\"\n").unwrap();
    fs::copy(udev.join("udev.h"), out.join("libudev.h")).unwrap();
    let headers: String = ["input-event-codes.h", "input.h"]
        .iter()
        .map(|name| fs::read_to_string(evdev.join("include/linux/linux").join(name)).unwrap())
        .collect();
    fs::write(out.join("event-names.h"), event_names::generate(&headers)).unwrap();

    // Immutable, relocation-safe data inside the NIF. No extraction, filesystem
    // writes or process-wide environment mutation is necessary at runtime.
    let quirks = files(&libinput.join("quirks"), "quirks");
    assert!(!quirks.is_empty());
    let strings: Vec<_> = quirks
        .iter()
        .map(|path| {
            let bytes = fs::read(path).unwrap();
            let data: String = bytes.iter().map(|b| format!("\\x{b:02x}")).collect();
            format!(
                "{{\"{}\", \"{}\"}}",
                path.file_name().unwrap().to_str().unwrap(),
                data
            )
        })
        .collect();
    fs::write(out.join("emerge-quirks.h"), format!("static const struct {{ const char *name; const char *data; }} emerge_quirks[] = {{ {} }};\n", strings.join(",\n"))).unwrap();
    patch(
        &input.join("quirks.c"),
        &out.join("quirks.c"),
        &[
            (
                "#include \"quirks.h\"",
                "#include \"quirks.h\"\n#include \"quirks-bundle.h\"",
            ),
            ("fp = fopen(path, \"r\");", "fp = emerge_quirks_open(path);"),
            (
                "ndev = scandir(data_path, &namelist, is_data_file, versionsort);",
                "if (strcmp(data_path, \"emerge:bundled\") == 0) return emerge_parse_quirks(ctx);\n\tndev = scandir(data_path, &namelist, is_data_file, versionsort);",
            ),
        ],
    );
    patch(
        &udev.join("udev_device.c"),
        &out.join("udev_device.c"),
        &[
            (
                "else if (test_bit(ev_bits, EV_ABS))",
                "if (test_bit(ev_bits, EV_ABS))",
            ),
            (
                "else if (test_bit(abs_bits, ABS_Y) && test_bit(abs_bits, ABS_X))",
                "else if ((test_bit(abs_bits, ABS_Y) && test_bit(abs_bits, ABS_X)) ||\n                 (test_bit(abs_bits, ABS_MT_POSITION_Y) && test_bit(abs_bits, ABS_MT_POSITION_X)))",
            ),
            (
                "else if (test_bit(key_bits, BTN_TOUCH))",
                "else if (test_bit(key_bits, BTN_TOUCH) || test_bit(prop_bits, INPUT_PROP_DIRECT))",
            ),
            (
                "if (test_bit(key_bits, BTN_TOOL_FINGER))",
                "if (test_bit(key_bits, BTN_TOOL_FINGER) && !test_bit(prop_bits, INPUT_PROP_DIRECT))",
            ),
        ],
    );
    let classified = out.join("udev_device.c");
    let classifier =
        fs::read_to_string(&classified).unwrap() + include_str!("classification-test.c");
    fs::write(classified, classifier).unwrap();
    let mut build = cc::Build::new();
    build
        .std("gnu11")
        .pic(true)
        .warnings(false)
        .flag_if_supported("-fvisibility=hidden")
        .define("_GNU_SOURCE", "1")
        .define("_XOPEN_SOURCE", "700")
        .include(&out)
        .include(&root)
        .include(&input)
        .include(libinput)
        .include(evdev)
        .include(evdev.join("include"))
        .include(mtdev.join("include"))
        .include(udev);
    build.files(
        files(&input, "c")
            .into_iter()
            .filter(|p| p.file_name().unwrap() != "quirks.c"),
    );
    build.file(out.join("quirks.c"));
    build.files(files(&evdev.join("libevdev"), "c"));
    build.files(files(&mtdev.join("src"), "c"));
    build.files(
        files(udev, "c")
            .into_iter()
            .filter(|p| p.file_name().unwrap() != "udev_device.c"),
    );
    build
        .file(out.join("udev_device.c"))
        .file(root.join("shim.c"));
    build.compile("emerge_input_bundle");
    println!("cargo:rustc-link-lib=m");
}
