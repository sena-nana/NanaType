//! Built-in fonts registered with the text engine for the active skin.
//!
//! A built-in skin names an embedded font as `file = "builtin:fonts/X.ttf"`
//! ([`nanatype_theme::builtin_font`]). Those are registered lazily, only
//! once a skin that uses them is selected, and once each: body, name and
//! accent naming the same font, or switching between two skins that share
//! it, keeps the one registration. A font the next skin does not use is
//! withdrawn. A reference this build does not embed registers nothing, and
//! the role falls back to its `families`.

use std::collections::HashMap;

/// A registered font: whatever keeps it registered (dropping it withdraws
/// the font) and the family it registered under.
struct Held<H> {
    _handle: H,
    family: Option<String>,
}

/// The built-in fonts currently registered, by `builtin:` reference.
pub struct BuiltinFontSet<H> {
    held: HashMap<String, Held<H>>,
}

impl<H> Default for BuiltinFontSet<H> {
    fn default() -> Self {
        Self {
            held: HashMap::new(),
        }
    }
}

impl<H> BuiltinFontSet<H> {
    /// Make `wanted` the registered set. Fonts already held stay as they
    /// are; new ones are looked up with `lookup` and handed to `register`;
    /// the rest are dropped. Returns registration errors.
    pub fn sync<'a>(
        &mut self,
        wanted: impl IntoIterator<Item = &'a str>,
        lookup: impl Fn(&str) -> Option<&'static [u8]>,
        mut register: impl FnMut(&'static [u8]) -> Result<(H, Option<String>), String>,
    ) -> Vec<String> {
        let mut want: Vec<&str> = wanted.into_iter().collect();
        want.sort_unstable();
        want.dedup();
        self.held.retain(|k, _| want.contains(&k.as_str()));
        let mut errors = Vec::new();
        for reference in want {
            if self.held.contains_key(reference) {
                continue;
            }
            // Not in this build: nothing to register, the families apply.
            let Some(bytes) = lookup(reference) else {
                continue;
            };
            match register(bytes) {
                Ok((handle, family)) => {
                    self.held.insert(
                        reference.to_owned(),
                        Held {
                            _handle: handle,
                            family,
                        },
                    );
                }
                Err(e) => errors.push(format!("{reference}: {e}")),
            }
        }
        errors
    }

    /// The family `reference` registered under, when it is registered.
    pub fn family(&self, reference: &str) -> Option<String> {
        self.held.get(reference).and_then(|h| h.family.clone())
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

/// Register `bytes` with NanaUI in a scope of its own, the way a skin
/// folder's fonts are registered.
pub fn register_with_nana_ui(
    bytes: &'static [u8],
) -> Result<(nana_ui::HostFontScope, Option<String>), String> {
    let mut scope = nana_ui::HostFontScope::new();
    let family = scope
        .add_bytes(bytes)
        .map_err(|e| e.to_string())?
        .families()
        .first()
        .cloned();
    Ok((scope, family))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    const NOTO: &str = "builtin:fonts/NotoSansSC-Regular.ttf";
    const OTHER: &str = "builtin:fonts/Other.ttf";
    const ABSENT: &str = "builtin:fonts/NotInThisBuild.ttf";

    /// The test build's table: Noto Sans SC from NanaUI, and a second font.
    fn lookup(reference: &str) -> Option<&'static [u8]> {
        match reference {
            NOTO => Some(nana_ui::UI_FONT_REGULAR),
            OTHER => Some(b"other"),
            _ => None,
        }
    }

    /// Counts registrations and how many are still alive.
    struct Probe {
        live: Rc<Cell<usize>>,
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.live.set(self.live.get() - 1);
        }
    }

    #[test]
    fn each_font_registers_once_and_unused_ones_are_withdrawn() {
        let (calls, live) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let mut set = BuiltinFontSet::default();
        let sync = |set: &mut BuiltinFontSet<Probe>, wanted: &[&str]| {
            set.sync(wanted.iter().copied(), lookup, |bytes| {
                calls.set(calls.get() + 1);
                live.set(live.get() + 1);
                let family = (bytes.len() > 100).then(|| "Noto Sans SC".to_owned());
                Ok((Probe { live: live.clone() }, family))
            })
        };
        // Body, name and accent all name Noto: one registration.
        assert!(sync(&mut set, &[NOTO, NOTO, NOTO]).is_empty());
        assert_eq!((calls.get(), live.get()), (1, 1));
        assert_eq!(set.family(NOTO).as_deref(), Some("Noto Sans SC"));
        // Another skin sharing Noto keeps it registered.
        sync(&mut set, &[NOTO, OTHER]);
        assert_eq!((calls.get(), live.get()), (2, 2));
        // A skin without Noto withdraws it; Other stays.
        sync(&mut set, &[OTHER]);
        assert_eq!((calls.get(), live.get()), (2, 1));
        assert_eq!(set.family(NOTO), None);
        // A skin with no built-in fonts withdraws everything.
        sync(&mut set, &[]);
        assert_eq!(live.get(), 0);
        assert!(set.is_empty());
    }

    #[test]
    fn a_font_missing_from_the_build_registers_nothing() {
        let mut set = BuiltinFontSet::<()>::default();
        let errors = set.sync([ABSENT], lookup, |_| panic!("nothing to register"));
        assert!(errors.is_empty());
        assert_eq!(set.family(ABSENT), None);
        assert!(set.is_empty());
    }

    #[test]
    fn a_failed_registration_is_reported_and_retried_next_time() {
        let mut set = BuiltinFontSet::<()>::default();
        let errors = set.sync([OTHER], lookup, |_| Err("bad font".into()));
        assert_eq!(errors, vec![format!("{OTHER}: bad font")]);
        assert!(set.is_empty());
        assert!(set.sync([OTHER], lookup, |_| Ok(((), None))).is_empty());
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn noto_sans_sc_registers_with_nana_ui_under_its_family() {
        let mut set = BuiltinFontSet::default();
        let errors = set.sync([NOTO], lookup, register_with_nana_ui);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(set.family(NOTO).as_deref(), Some("Noto Sans SC"));
        // Selecting it again does not register a second copy.
        let errors = set.sync([NOTO], lookup, |_| panic!("registered twice"));
        assert!(errors.is_empty());
    }
}
