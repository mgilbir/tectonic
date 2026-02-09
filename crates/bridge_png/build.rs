//! `libpng` build script.

use tectonic_dep_support::{Configuration, Dependency, Spec};

struct LibpngSpec;

impl Spec for LibpngSpec {
    fn get_pkgconfig_spec(&self) -> &str {
        "libpng"
    }

    fn get_vcpkg_spec(&self) -> &[&str] {
        &["libpng"]
    }

    fn get_manual_env_prefix(&self) -> String {
        "PNG".into()
    }

    fn get_manual_link_libs(&self) -> Vec<String> {
        vec!["png16".into()]
    }
}

fn main() {
    // Find any necessary deps.

    let dep_cfg = Configuration::default();
    let dep = Dependency::probe(LibpngSpec, &dep_cfg);

    // This is the key. What we print here will be propagated into depending
    // crates' build scripts as the environment variable DEP_LIBPNG_INCLUDE_PATH,
    // allowing them to find the headers internally. If/when we start vendoring
    // libPNG, this can become $OUT_DIR.
    let mut sep = "cargo:include-path=";

    dep.foreach_include_path(|p| {
        print!("{}{}", sep, p.to_str().unwrap());
        sep = ";";
    });

    println!();

    dep.emit();
}
