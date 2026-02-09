// Copyright 2020 the Tectonic Project
// Licensed under the MIT License.

//! Support for locating third-party libraries ("dependencies") when building
//! Tectonic. The main point of interest is that both pkg-config and vcpkg are
//! supported as dep-finding backends. This crate does *not* deal with the
//! choice of whether to provide a library externally or through vendoring.

use std::{
    env,
    path::{Path, PathBuf},
};

/// Supported depedency-finding backends.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Backend {
    /// pkg-config
    #[default]
    PkgConfig,

    /// vcpkg
    Vcpkg,

    /// Manual specification via environment variables (used for WASI cross-compilation)
    Manual,
}

/// Dep-finding configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Configuration {
    /// The dep-finding backend being used.
    pub backend: Backend,

    semi_static_mode: bool,
}

impl Default for Configuration {
    /// This default function will fetch settings from the environment. Is that a no-no?
    fn default() -> Self {
        println!("cargo:rerun-if-env-changed=TECTONIC_DEP_BACKEND");
        println!("cargo:rerun-if-env-changed=TECTONIC_PKGCONFIG_FORCE_SEMI_STATIC");

        // This should use FromStr or whatever, but meh.
        let backend = if let Ok(dep_backend_str) = env::var("TECTONIC_DEP_BACKEND") {
            match dep_backend_str.as_ref() {
                "pkg-config" => Backend::PkgConfig,
                "vcpkg" => Backend::Vcpkg,
                "manual" => Backend::Manual,
                "default" => Backend::default(),
                other => panic!("unrecognized TECTONIC_DEP_BACKEND setting {other:?}"),
            }
        } else if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
            Backend::Manual
        } else {
            Backend::default()
        };

        let semi_static_mode = env::var("TECTONIC_PKGCONFIG_FORCE_SEMI_STATIC").is_ok();

        Configuration {
            backend,
            semi_static_mode,
        }
    }
}

/// Information specifying a dependency.
pub trait Spec {
    /// Get the pkg-config specification used to check for this dependency. This
    /// text will be passed into `pkg_config::Config::probe()`.
    fn get_pkgconfig_spec(&self) -> &str;

    /// Get the vcpkg packages used to check for this dependency. These will be
    /// passed into `vcpkg::Config::find_package()`.
    fn get_vcpkg_spec(&self) -> &[&str];

    /// Get the environment variable name prefix for the manual backend.
    /// For example, "FREETYPE2" would look for FREETYPE2_INCLUDE_PATH and FREETYPE2_LIB_DIR.
    /// Defaults to the uppercased pkg-config spec with hyphens replaced by underscores.
    fn get_manual_env_prefix(&self) -> String {
        self.get_pkgconfig_spec()
            .to_uppercase()
            .replace('-', "_")
            .replace('+', "_")
    }

    /// Get the static library names to link for the manual backend.
    /// Defaults to a single library matching the pkg-config spec.
    fn get_manual_link_libs(&self) -> Vec<String> {
        vec![self.get_pkgconfig_spec().replace("-", "").replace("+", "")]
    }
}

/// Build-script state when using pkg-config as the backend.
#[derive(Debug)]
struct PkgConfigState {
    libs: pkg_config::Library,
}

/// Build-script state when using vcpkg as the backend.
#[derive(Clone, Debug)]
struct VcPkgState {
    include_paths: Vec<PathBuf>,
}

/// Build-script state when using manual env-var specification.
#[derive(Clone, Debug)]
struct ManualState {
    include_paths: Vec<PathBuf>,
    lib_dirs: Vec<PathBuf>,
    link_libs: Vec<String>,
}

/// State for discovering and managing a dependency, which may vary
/// depending on the framework that we're using to discover them.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum DepState {
    /// pkg-config
    PkgConfig(PkgConfigState),

    /// vcpkg
    VcPkg(VcPkgState),

    /// Manual env-var specification
    Manual(ManualState),
}

impl DepState {
    /// Probe the dependency.
    fn new<T: Spec>(spec: &T, config: &Configuration) -> Self {
        match config.backend {
            Backend::PkgConfig => DepState::new_from_pkg_config(spec, config),
            Backend::Vcpkg => DepState::new_from_vcpkg(spec, config),
            Backend::Manual => DepState::new_from_manual(spec),
        }
    }

    /// Probe using pkg-config.
    fn new_from_pkg_config<T: Spec>(spec: &T, config: &Configuration) -> Self {
        let libs = pkg_config::Config::new()
            .cargo_metadata(false)
            .statik(config.semi_static_mode)
            .probe(spec.get_pkgconfig_spec())
            .unwrap();

        DepState::PkgConfig(PkgConfigState { libs })
    }

    /// Probe using vcpkg.
    fn new_from_vcpkg<T: Spec>(spec: &T, _config: &Configuration) -> Self {
        let mut include_paths = vec![];

        for dep in spec.get_vcpkg_spec() {
            let library = match vcpkg::Config::new().cargo_metadata(false).find_package(dep) {
                Ok(lib) => lib,
                Err(e) => {
                    if let vcpkg::Error::LibNotFound(_) = e {
                        // We should potentially be referencing the CARGO_CFG_TARGET_*
                        // variables to handle cross-compilation (cf. the
                        // tectonic_cfg_support crate), but vcpkg-rs doesn't use them
                        // either.
                        let target = env::var("TARGET").unwrap_or_default();

                        if target == "x86_64-pc-windows-msvc" {
                            println!("cargo:warning=you may need to export VCPKGRS_TRIPLET=x64-windows-static-release ...");
                            println!("cargo:warning=... which is a custom triplet used by Tectonic's cargo-vcpkg integration");
                        }
                    }

                    panic!("failed to load package {dep} from vcpkg: {e}")
                }
            };

            include_paths.extend(library.include_paths.iter().cloned());
        }

        DepState::VcPkg(VcPkgState { include_paths })
    }

    /// Probe using manual environment variables.
    fn new_from_manual<T: Spec>(spec: &T) -> Self {
        let prefix = spec.get_manual_env_prefix();

        let include_var = format!("{prefix}_INCLUDE_PATH");
        let lib_var = format!("{prefix}_LIB_DIR");

        println!("cargo:rerun-if-env-changed={include_var}");
        println!("cargo:rerun-if-env-changed={lib_var}");

        let include_paths = match env::var(&include_var) {
            Ok(val) => val
                .split(';')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect(),
            Err(_) => vec![],
        };

        let lib_dirs = match env::var(&lib_var) {
            Ok(val) => val
                .split(';')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect(),
            Err(_) => vec![],
        };

        let link_libs = spec.get_manual_link_libs();

        DepState::Manual(ManualState {
            include_paths,
            lib_dirs,
            link_libs,
        })
    }
}

/// A dependency.
pub struct Dependency<'a, T: Spec> {
    config: &'a Configuration,
    spec: T,
    state: DepState,
}

impl<'a, T: Spec> Dependency<'a, T> {
    /// Probe the dependency.
    pub fn probe(spec: T, config: &'a Configuration) -> Self {
        let state = DepState::new(&spec, config);

        Dependency {
            config,
            spec,
            state,
        }
    }

    /// Invoke a callback for each C/C++ include directory injected by our
    /// dependencies.
    pub fn foreach_include_path<F>(&self, mut f: F)
    where
        F: FnMut(&Path),
    {
        match self.state {
            DepState::PkgConfig(ref s) => {
                for p in &s.libs.include_paths {
                    f(p);
                }
            }

            DepState::VcPkg(ref s) => {
                for p in &s.include_paths {
                    f(p);
                }
            }

            DepState::Manual(ref s) => {
                for p in &s.include_paths {
                    f(p);
                }
            }
        }
    }

    /// Emit build information about this dependency. This should be called
    /// after all information for in-crate builds is emitted.
    pub fn emit(&self) {
        match self.state {
            DepState::PkgConfig(ref state) => {
                if self.config.semi_static_mode {
                    // pkg-config will prevent "system libraries" from being
                    // linked statically even when PKG_CONFIG_ALL_STATIC=1,
                    // but its definition of a system library isn't always
                    // perfect. For Debian cross builds, we'd like to make
                    // binaries that are dynamically linked with things like
                    // libc and libm but not libharfbuzz, etc. In this mode we
                    // override pkg-config's logic by emitting the metadata
                    // ourselves.
                    for link_path in &state.libs.link_paths {
                        println!("cargo:rustc-link-search=native={}", link_path.display());
                    }

                    for fw_path in &state.libs.framework_paths {
                        println!("cargo:rustc-link-search=framework={}", fw_path.display());
                    }

                    for libbase in &state.libs.libs {
                        let do_static = match libbase.as_ref() {
                            "c" | "m" | "dl" | "pthread" => false,
                            _ => {
                                // Frustratingly, graphite2 seems to have
                                // issues with static builds; e.g. static
                                // graphite2 is not available on Debian. So
                                // let's jump through the hoops of testing
                                // whether the static archive seems findable.
                                let libname = format!("lib{libbase}.a");
                                state
                                    .libs
                                    .link_paths
                                    .iter()
                                    .any(|d| d.join(&libname).exists())
                            }
                        };

                        let mode = if do_static { "static=" } else { "" };
                        println!("cargo:rustc-link-lib={mode}{libbase}");
                    }

                    for fw in &state.libs.frameworks {
                        println!("cargo:rustc-link-lib=framework={fw}");
                    }
                } else {
                    // Just let pkg-config do its thing.
                    pkg_config::Config::new()
                        .cargo_metadata(true)
                        .probe(self.spec.get_pkgconfig_spec())
                        .unwrap();
                }
            }

            DepState::VcPkg(_) => {
                for dep in self.spec.get_vcpkg_spec() {
                    vcpkg::find_package(dep)
                        .unwrap_or_else(|e| panic!("failed to load package {dep} from vcpkg: {e}"));
                }
            }

            DepState::Manual(ref state) => {
                for lib_dir in &state.lib_dirs {
                    println!("cargo:rustc-link-search=native={}", lib_dir.display());
                }
                for lib in &state.link_libs {
                    println!("cargo:rustc-link-lib=static={lib}");
                }
            }
        }
    }
}
