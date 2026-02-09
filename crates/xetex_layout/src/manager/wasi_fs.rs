// Copyright 2024 the Tectonic Project
// Licensed under the MIT License.

//! WASI filesystem-based font backend.
//!
//! In WASI, there is no fontconfig or CoreText. Instead, we scan a mounted
//! font directory for .ttf, .otf, and .ttc files and use FreeType to
//! extract font metadata.

use super::{
    base_get_op_size_rec_and_style_flags, FontInfo, FontManager, FontManagerBackend, FontMaps,
    NameCollection,
};
use crate::c_api::{PlatformFontRef, WasiFontRef};
use enrede::encoding::{MacRoman, Utf16BE, Utf8};
use enrede::Str;
use std::borrow::Cow;
use std::ffi::{CStr, CString};
use tectonic_bridge_freetype2 as ft;

const FONT_FAMILY_NAME: libc::c_ushort = 1;
const FONT_STYLE_NAME: libc::c_ushort = 2;
const FONT_FULL_NAME: libc::c_ushort = 4;
const PREFERRED_FAMILY_NAME: libc::c_ushort = 16;
const PREFERRED_SUBFAMILY_NAME: libc::c_ushort = 17;

/// Font backend that scans a directory for font files.
pub struct WasiFsBackend {
    all_fonts: Vec<PlatformFontRef>,
    cached_all: bool,
}

impl WasiFsBackend {
    pub fn new() -> WasiFsBackend {
        ft::init();

        let font_dir = std::env::var("TECTONIC_FONT_DIR").unwrap_or_else(|_| "/fonts".into());
        let mut all_fonts = Vec::new();

        Self::scan_directory(&font_dir, &mut all_fonts);

        WasiFsBackend {
            all_fonts,
            cached_all: false,
        }
    }

    fn scan_directory(dir: &str, fonts: &mut Vec<PlatformFontRef>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            if path.is_dir() {
                if let Some(s) = path.to_str() {
                    Self::scan_directory(s, fonts);
                }
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());

            match ext.as_deref() {
                Some("ttf") | Some("otf") | Some("ttc") | Some("dfont") => {}
                _ => continue,
            }

            let path_str = match path.to_str() {
                Some(s) => s,
                None => continue,
            };

            let c_path = match CString::new(path_str) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Try to open the font to get the number of faces
            let face = match ft::Face::new(&c_path, 0) {
                Ok(face) => face,
                Err(_) => continue,
            };

            let num_faces = face.num_faces();
            drop(face);

            for i in 0..num_faces {
                fonts.push(WasiFontRef {
                    path: c_path.clone(),
                    index: i as i32,
                });
            }
        }
    }

    fn cache_family_members(&mut self, maps: &mut FontMaps, names: &[CString]) {
        if names.is_empty() {
            return;
        }

        for font_ref in &self.all_fonts {
            if maps.platform_ref_to_font.contains_key(font_ref) {
                continue;
            }

            let font_names = self.read_names(font_ref.clone());
            for family in &font_names.family_names {
                if names.iter().any(|n| **n == **family) {
                    maps.add_to_maps(self, font_ref.clone(), &font_names);
                    break;
                }
            }
        }
    }
}

impl FontManagerBackend for WasiFsBackend {
    fn get_platform_font_desc<'a>(&'a self, font: &'a PlatformFontRef) -> Cow<'a, CStr> {
        Cow::Borrowed(&font.path)
    }

    fn get_op_size_rec_and_style_flags(&self, font: &mut FontInfo) {
        base_get_op_size_rec_and_style_flags(font);
    }

    fn search_for_host_platform_fonts(&mut self, maps: &mut FontMaps, name: &CStr) {
        if self.cached_all {
            return;
        }

        let bytes = name.to_bytes();
        let split = bytes
            .iter()
            .position(|c| *c == b'-')
            .map(|index| (&bytes[..index], &bytes[index + 1..]));

        let (fam_name, hyph) = match split {
            Some((fam, _)) => (fam, fam.len()),
            None => (&[] as &[_], 0),
        };

        let mut found = false;
        loop {
            for i in 0..self.all_fonts.len() {
                let font_ref = self.all_fonts[i].clone();
                if maps.platform_ref_to_font.contains_key(&font_ref) {
                    if !self.cached_all {
                        continue;
                    }
                }

                if self.cached_all {
                    let names = self.read_names(font_ref.clone());
                    maps.add_to_maps(self, font_ref, &names);
                    continue;
                }

                let names = self.read_names(font_ref.clone());

                // Check full names
                for full in &names.full_names {
                    if name == full.as_c_str() {
                        maps.add_to_maps(self, font_ref.clone(), &names);
                        self.cache_family_members(maps, &names.family_names);
                        found = true;
                        break;
                    }
                }
                if found {
                    continue;
                }

                // Check family names
                for fam in &names.family_names {
                    if name == fam.as_c_str()
                        || (hyph != 0 && fam_name == fam.to_bytes())
                    {
                        maps.add_to_maps(self, font_ref.clone(), &names);
                        self.cache_family_members(maps, &names.family_names);
                        found = true;
                        break;
                    }

                    // Try "Family Style" combination
                    for style in &names.style_names {
                        let mut full = fam.to_bytes().to_owned();
                        full.push(b' ');
                        full.extend(style.to_bytes());
                        if name.to_bytes() == full {
                            maps.add_to_maps(self, font_ref.clone(), &names);
                            self.cache_family_members(maps, &names.family_names);
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
            }

            if found || self.cached_all {
                break;
            }
            self.cached_all = true;
        }
    }

    fn read_names(&self, font_ref: PlatformFontRef) -> NameCollection {
        let mut names = NameCollection::default();

        let face = match ft::Face::new(&font_ref.path, font_ref.index as usize) {
            Ok(face) => face,
            Err(_) => return names,
        };

        let ps_name = match face.get_postscript_name() {
            Some(name) => name,
            None => return names,
        };

        names.ps_name = Some(ps_name.to_owned());

        if face.is_sfnt() {
            let mut family_names = Vec::new();
            let mut sub_family_names = Vec::new();

            for i in 0..face.get_sfnt_name_count() {
                let mut utf8_name = None;
                let name_rec = match face.get_sfnt_name(i) {
                    Ok(name) => name,
                    Err(_) => continue,
                };

                match name_rec.name_id {
                    FONT_FULL_NAME
                    | FONT_FAMILY_NAME
                    | FONT_STYLE_NAME
                    | PREFERRED_FAMILY_NAME
                    | PREFERRED_SUBFAMILY_NAME => {
                        let mut preferred_name = false;
                        if name_rec.platform_id == ft::PlatformId::MACINTOSH
                            && name_rec.encoding_id == ft::EncodingId::MAC_ROMAN
                            && name_rec.language_id == ft::LanguageId::MAC_ENGLISH
                        {
                            let str = Str::<MacRoman>::from_bytes_infallible(name_rec.string);
                            utf8_name = Some(
                                enrede::CString::try_from(str.recode::<Utf8>().unwrap()).unwrap(),
                            );
                            preferred_name = true;
                        } else if name_rec.platform_id == ft::PlatformId::APPLE_UNICODE
                            || name_rec.platform_id == ft::PlatformId::MICROSOFT
                        {
                            let str = Str::<Utf16BE>::from_bytes(name_rec.string).unwrap();
                            utf8_name = Some(
                                enrede::CString::try_from(str.recode::<Utf8>().unwrap()).unwrap(),
                            );
                        }

                        if let Some(name) = utf8_name {
                            let name_list = match name_rec.name_id {
                                FONT_FULL_NAME => &mut names.full_names,
                                FONT_FAMILY_NAME => &mut names.family_names,
                                FONT_STYLE_NAME => &mut names.style_names,
                                PREFERRED_FAMILY_NAME => &mut family_names,
                                PREFERRED_SUBFAMILY_NAME => &mut sub_family_names,
                                _ => unreachable!(),
                            };

                            if preferred_name {
                                FontManager::prepend_to_list(name_list, name.into_std());
                            } else {
                                FontManager::append_to_list(name_list, name.into_std());
                            }
                        }
                    }
                    _ => (),
                }
            }
        } else {
            // For non-SFNT fonts, use the family and style from raw FreeType fields
            unsafe {
                let raw = face.raw();
                if !(*raw).family_name.is_null() {
                    let family = CStr::from_ptr((*raw).family_name);
                    FontManager::append_to_list(&mut names.family_names, family.to_owned());
                }
                if !(*raw).style_name.is_null() {
                    let style = CStr::from_ptr((*raw).style_name);
                    FontManager::append_to_list(&mut names.style_names, style.to_owned());
                }
            }
        }

        names
    }
}
