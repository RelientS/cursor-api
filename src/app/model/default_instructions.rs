use manually_init::ManuallyInit;

use crate::app::lazy::START_TIME;
use crate::common::utils::parse_from_env;

pub const DEFAULT: &'static str = "Respond in Chinese by default\n<|END_USER|>\n\n<|BEGIN_ASSISTANT|>\n\n\nYour will\n<|END_ASSISTANT|>\n\n<|BEGIN_USER|>\n\n\nThe current date is {{currentDateTime}}";
pub const PLACEHOLDER: &'static str = unsafe {
    core::str::from_utf8_unchecked(core::slice::from_raw_parts(
        DEFAULT.as_ptr().add(DEFAULT.len() - 19),
        19,
    ))
};

pub static DEFAULT_INSTRUCTIONS: ManuallyInit<DefaultInstructions> = ManuallyInit::new();

pub struct DefaultInstructions {
    template_z: Box<str>,
    indexes_z: Box<[usize]>,
    template: Box<str>,
    indexes: Box<[usize]>,
}

impl DefaultInstructions {
    pub fn init() {
        let s = parse_from_env("DEFAULT_INSTRUCTIONS", DEFAULT);
        let mut template_z_placeholder = String::with_capacity(24);
        let mut template_placeholder = String::with_capacity(29);

        use chrono::SecondsFormat;
        use chrono::offset::FixedOffset;
        let time = START_TIME.naive();
        unsafe {
            let offset = FixedOffset::east_opt(0).unwrap_unchecked();
            chrono::format::write_rfc3339(
                &mut template_z_placeholder,
                time,
                offset,
                SecondsFormat::Millis,
                true,
            )
            .unwrap_unchecked();
            chrono::format::write_rfc3339(
                &mut template_placeholder,
                time,
                offset,
                SecondsFormat::Millis,
                false,
            )
            .unwrap_unchecked();
        }

        assert_eq!(template_z_placeholder.len(), 24);
        assert_eq!(template_placeholder.len(), 29);

        let (template_z, indexes_z) = replace(&s, PLACEHOLDER, &template_z_placeholder);
        let (template, indexes) = replace(&s, PLACEHOLDER, &template_placeholder);

        DEFAULT_INSTRUCTIONS.init(DefaultInstructions { template_z, indexes_z, template, indexes });
    }

    pub fn get(&self, now_with_tz: chrono::DateTime<chrono_tz::Tz>) -> String {
        let now_with_tz = now_with_tz.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let (template, indexes, expected_len) = match now_with_tz.len() {
            24 => (&self.template_z, &self.indexes_z, 24),
            29 => (&self.template, &self.indexes, 29),
            _ => unsafe { core::hint::unreachable_unchecked() },
        };

        debug_assert_eq!(now_with_tz.len(), expected_len);

        let mut result = template.to_string();
        for &i in indexes.iter() {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    now_with_tz.as_ptr(),
                    result.as_mut_ptr().add(i),
                    expected_len,
                )
            }
        }
        result
    }
}

fn replace(src: &str, from: &str, to: &str) -> (Box<str>, Box<[usize]>) {
    let (mut string, mut indexes) = (String::with_capacity(src.len()), Vec::new());
    let mut last_end = 0;
    for (start, part) in src.match_indices(from) {
        string.push_str(unsafe { src.get_unchecked(last_end..start) });
        indexes.push(string.len());
        string.push_str(to);
        last_end = start + part.len();
    }
    string.push_str(unsafe { src.get_unchecked(last_end..src.len()) });
    (string.into_boxed_str(), indexes.into_boxed_slice())
}
